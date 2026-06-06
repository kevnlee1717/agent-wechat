//! MediaPoller：扫描 message_*.db 中图片/视频消息，尝试解密与落盘，发出 message.media_ready 事件。

use crate::db::get_db;
use crate::events::{bus::EventBus, kinds, Event};
use crate::events::pollers::Poller;
use crate::sessions::manager::get_session;
use crate::tools::wechat_chats;
use crate::tools::wechat_db::{find_wechat_pid, list_account_dbs};
use crate::tools::wechat_keys::{extract_keys_async, get_image_keys, get_stored_keys, store_keys};
use crate::tools::wechat_media::{get_message_media, original_dat_exists};
use crate::tools::wechat_messages;
use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;

static UI_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub struct MediaPoller {
    pub last_seen: HashMap<String, i64>,
    pub interval_ms: u64,
    pub cold_start: bool,
    pub media_dir: PathBuf,
    pub image_fetch_original: bool,
    pub image_fetch_timeout_ms: u64,
}

impl MediaPoller {
    pub fn new(
        interval_ms: u64,
        media_dir: PathBuf,
        image_fetch_original: bool,
        image_fetch_timeout_ms: u64,
    ) -> Self {
        Self {
            last_seen: HashMap::new(),
            interval_ms,
            cold_start: true,
            media_dir,
            image_fetch_original,
            image_fetch_timeout_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PollAction {
    Retry,
    UpdateCursor,
    Emit,
}

fn is_media_message_type(msg_type: i32) -> bool {
    matches!(msg_type, 3 | 43)
}

fn message_create_time(timestamp: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.timestamp())
}

async fn wait_for_original_image(
    account_dir: &str,
    keys: &HashMap<String, String>,
    chat_id: &str,
    local_id: i64,
    create_time: i64,
    timeout_ms: u64,
) {
    if original_dat_exists(account_dir, keys, chat_id, local_id, create_time) {
        return;
    }

    let _guard = UI_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .await;

    if original_dat_exists(account_dir, keys, chat_id, local_id, create_time) {
        return;
    }

    let open_result = crate::tools::chat_select::open_chat(chat_id, false, None).await;
    if !open_result.ok {
        tracing::warn!(
            chat_id = %chat_id,
            local_id,
            error = ?open_result.error,
            "open_chat failed while fetching original image"
        );
    }

    let mut waited = 0u64;
    while waited < timeout_ms {
        tokio::time::sleep(Duration::from_millis(500)).await;
        waited += 500;
        if original_dat_exists(account_dir, keys, chat_id, local_id, create_time) {
            tracing::info!(
                "[media] original dat landed local_id={} after {}ms",
                local_id,
                waited
            );
            return;
        }
    }

    tracing::warn!(
        "[media] original fetch timeout, will fall back to thumbnail local_id={}",
        local_id
    );
}

fn decide_media_action(media_type: &str, has_data: bool, cold_start: bool) -> PollAction {
    if cold_start {
        return PollAction::UpdateCursor;
    }

    match media_type {
        "pending" => PollAction::Retry,
        "unsupported" => PollAction::UpdateCursor,
        _ if !has_data => PollAction::UpdateCursor,
        _ => PollAction::Emit,
    }
}

fn media_ready_event(
    chat_id: &str,
    local_id: i64,
    media_type: &str,
    format: &str,
    container_path: &str,
    relative_path: &str,
    size_bytes: usize,
) -> Event {
    Event::new(
        kinds::MESSAGE_MEDIA_READY,
        json!({
            "chatId": chat_id,
            "localId": local_id,
            "mediaType": media_type,
            "format": format,
            "sizeBytes": size_bytes,
            "containerPath": container_path,
            "relativePath": relative_path,
        }),
    )
}

#[async_trait]
impl Poller for MediaPoller {
    fn name(&self) -> &'static str {
        "media"
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }

    async fn poll(&mut self, bus: &EventBus) -> Result<()> {
        let session = match get_session("default") {
            Some(s) => s,
            None => return Ok(()),
        };

        let logged_in_user = match session.logged_in_user.as_ref() {
            Some(u) => u.clone(),
            None => return Ok(()),
        };

        let mut keys = {
            let db = get_db();
            get_stored_keys(&db, &session.id, &logged_in_user)
        };

        let on_disk = list_account_dbs(&logged_in_user);
        let has_missing_message_db = on_disk
            .iter()
            .any(|name| {
                name.starts_with("message_")
                    && name.ends_with(".db")
                    && !name.contains("fts")
                    && !name.contains("resource")
                    && !keys.contains_key(name.as_str())
            });
        if has_missing_message_db {
            if let Some(pid) = find_wechat_pid() {
                let extracted = extract_keys_async(pid).await;
                if !extracted.is_empty() {
                    let db = get_db();
                    store_keys(&db, &session.id, &logged_in_user, &extracted);
                    keys = get_stored_keys(&db, &session.id, &logged_in_user);
                }
            }
        }

        let image_keys = {
            let db = get_db();
            get_image_keys(&db, &session.id, &logged_in_user)
        };

        if !keys
            .keys()
            .any(|k| k.starts_with("message_") && k.ends_with(".db") && !k.contains("fts") && !k.contains("resource"))
        {
            return Ok(());
        }

        let chats = wechat_chats::list_chats(&logged_in_user, &keys, 100, 0);
        for chat in chats {
            let chat_id = chat.id;
            let mut messages = wechat_messages::list_messages(
                &logged_in_user,
                &keys,
                &chat_id,
                50,
                0,
            );
            messages.sort_by_key(|m| m.local_id);

            let mut latest_seen = self.last_seen.get(&chat_id).cloned().unwrap_or(0);
            let mut updated = false;

            for msg in messages {
                if !is_media_message_type(msg.msg_type) || msg.local_id <= latest_seen {
                    continue;
                }

                if self.cold_start {
                    latest_seen = msg.local_id;
                    updated = true;
                    continue;
                }

                if msg.msg_type == 3 && self.image_fetch_original {
                    if let Some(create_time) = message_create_time(&msg.timestamp) {
                        wait_for_original_image(
                            &logged_in_user,
                            &keys,
                            &chat_id,
                            msg.local_id,
                            create_time,
                            self.image_fetch_timeout_ms,
                        )
                        .await;
                    } else {
                        tracing::warn!(
                            chat_id = %chat_id,
                            local_id = msg.local_id,
                            timestamp = %msg.timestamp,
                            "message timestamp unavailable, skip original image prefetch"
                        );
                    }
                }

                let media = get_message_media(
                    &logged_in_user,
                    &keys,
                    &chat_id,
                    msg.local_id,
                    image_keys.clone(),
                );
                let action = decide_media_action(
                    &media.media_type,
                    media.data.as_ref().is_some(),
                    self.cold_start,
                );

                match action {
                    PollAction::Retry => {
                        tracing::debug!(
                            chat_id = %chat_id,
                            local_id = msg.local_id,
                            "media pending, skip and retry next tick"
                        );
                        break;
                    }
                    PollAction::UpdateCursor => {
                        if media.media_type == "unsupported" {
                            tracing::warn!(
                                chat_id = %chat_id,
                                local_id = msg.local_id,
                                media_type = %media.media_type,
                                "media unsupported, skip and update cursor"
                            );
                        } else {
                            tracing::warn!(
                                chat_id = %chat_id,
                                local_id = msg.local_id,
                                media_type = %media.media_type,
                                "media data missing, skip and update cursor"
                            );
                        }
                        latest_seen = msg.local_id;
                        updated = true;
                    }
                    PollAction::Emit => {
                        let data_b64 = media.data.as_ref().expect("emit requires data");
                        let decoded = match STANDARD.decode(data_b64) {
                            Ok(bytes) => bytes,
                            Err(err) => {
                                tracing::error!(
                                    chat_id = %chat_id,
                                    local_id = msg.local_id,
                                    error = %err,
                                    "media decode failed"
                                );
                                continue;
                            }
                        };

                        let relative_path = format!("{chat_id}/{}.{}", msg.local_id, media.format);
                        let output_path = self.media_dir.join(&relative_path);
                        if let Some(dir) = output_path.parent() {
                            if let Err(err) = fs::create_dir_all(dir) {
                                tracing::error!(
                                    chat_id = %chat_id,
                                    local_id = msg.local_id,
                                    format = %media.format,
                                    error = %err,
                                    "create media dir failed"
                                );
                                continue;
                            }
                        }
                        if let Err(err) = fs::write(&output_path, &decoded) {
                            tracing::error!(
                                chat_id = %chat_id,
                                local_id = msg.local_id,
                                error = %err,
                                "media write failed"
                            );
                            continue;
                        }
                        let container_path = output_path.to_string_lossy().into_owned();
                        bus.publish(media_ready_event(
                            &chat_id,
                            msg.local_id,
                            &media.media_type,
                            &media.format,
                            &container_path,
                            &relative_path,
                            decoded.len(),
                        ));
                        latest_seen = msg.local_id;
                        updated = true;
                    }
                }
            }

            if updated || self.last_seen.contains_key(&chat_id) {
                self.last_seen.insert(chat_id, latest_seen);
            }
        }

        if self.cold_start {
            self.cold_start = false;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_ready_event_payload_shape() {
        let event = media_ready_event(
            "56486886448@chatroom",
            42,
            "image",
            "jpeg",
            "/wechat-media/56486886448@chatroom/42.jpeg",
            "56486886448@chatroom/42.jpeg",
            1024,
        );
        let expected = json!({
            "chatId": "56486886448@chatroom",
            "localId": 42,
            "mediaType": "image",
            "format": "jpeg",
            "sizeBytes": 1024,
            "containerPath": "/wechat-media/56486886448@chatroom/42.jpeg",
            "relativePath": "56486886448@chatroom/42.jpeg",
        });

        assert_eq!(event.kind, kinds::MESSAGE_MEDIA_READY);
        assert_eq!(event.data, expected);
    }

    #[test]
    fn media_poller_cold_start_advances_cursor_without_emit() {
        let decision = decide_media_action("pending", false, true);
        assert_eq!(decision, PollAction::UpdateCursor);
    }
}
