//! MessagePoller：按 chat 轮询微信 SQLite 消息库并发出事件。

use crate::db::get_db;
use crate::events::{bus::EventBus, kinds, Event};
use crate::ia::types::Message;
use crate::sessions::manager::get_session;
use crate::tools::wechat_chats;
use crate::tools::wechat_db::{find_wechat_pid, list_account_dbs};
use crate::tools::wechat_keys::{extract_keys_async, get_stored_keys, store_keys};
use crate::tools::wechat_messages;
use crate::events::pollers::Poller;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;
use serde_json::json;

/// 消息轮询器。
/// 冷启动时只记录游标，不发消息，避免重放历史消息。
/// 游标用 local_id（每个 Msg_ 表的 INTEGER PRIMARY KEY AUTOINCREMENT）—— 真单调。
/// 不要用 server_id：WeChat 实测 server_id 不单调（新消息可能小于旧消息），会漏推。
pub struct MessagePoller {
    /// chat_id -> 最后一次看到的 local_id。
    pub last_seen: HashMap<String, i64>,
    pub interval_ms: u64,
    pub cold_start: bool,
}

impl MessagePoller {
    pub fn new(interval_ms: u64) -> Self {
        Self {
            last_seen: HashMap::new(),
            interval_ms,
            cold_start: true,
        }
    }
}

#[async_trait]
impl Poller for MessagePoller {
    fn name(&self) -> &'static str {
        "message"
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

        // 与 router/messages.rs 保持一致：如果磁盘有新增 db 文件且未缓存密钥，按需提取。
        let on_disk = list_account_dbs(&logged_in_user);
        let has_missing_message_db = on_disk.iter().any(|name| {
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
                // 用 local_id 而不是 server_id —— server_id 实测非单调（WeChat 服务端分配规律不明）
                if msg.local_id <= latest_seen {
                    continue;
                }
                latest_seen = msg.local_id;
                updated = true;

                if self.cold_start {
                    continue;
                }

                bus.publish(message_event(&msg));
                if msg.msg_type == 10000 {
                    if let Some(evt) = derive_chatroom_event(&chat_id, &msg.content) {
                        bus.publish(evt);
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

fn message_event(message: &Message) -> Event {
    Event::new(
        kinds::MESSAGE_NEW,
        json!({
            "chatId": message.chat_id.clone(),
            "sender": message.sender.clone().unwrap_or_default(),
            "senderName": message.sender_name.clone().unwrap_or_default(),
            "msgType": message.msg_type,
            "content": message.content.clone(),
            "serverId": message.server_id.to_string(),
            "localId": message.local_id,
            "isSelf": message.is_self.unwrap_or(false),
            "timestamp": message.timestamp.clone(),
        }),
    )
}

/// 从群系统消息内容派生群成员变更事件。
/// 只处理 chatroom 且包含关键文案的系统消息。
pub fn derive_chatroom_event(chat_id: &str, content: &str) -> Option<Event> {
    if !chat_id.ends_with("@chatroom") {
        return None;
    }

    if let Some(name) = extract_name_before(content, "加入了群聊") {
        return Some(Event::new(
            kinds::CHATROOM_MEMBER_JOINED,
            json!({
                "chatId": chat_id,
                "wxid": "",
                "name": name,
            }),
        ));
    }

    if let Some(name) = extract_name_before(content, "退出了群聊") {
        return Some(Event::new(
            kinds::CHATROOM_MEMBER_LEFT,
            json!({
                "chatId": chat_id,
                "wxid": "",
                "name": name,
            }),
        ));
    }

    None
}

fn extract_name_before(content: &str, marker: &str) -> Option<String> {
    content
        .split_once(marker)
        .map(|(prefix, _)| prefix.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_chatroom_event_member_joined() {
        let evt = derive_chatroom_event("abc@chatroom", "wxid_xxx加入了群聊");
        assert!(evt.is_some());
        let evt = evt.expect("joined event");
        assert_eq!(evt.kind, kinds::CHATROOM_MEMBER_JOINED);
        assert_eq!(evt.chat_id().unwrap_or_default(), "abc@chatroom");
    }

    #[test]
    fn derive_chatroom_event_member_left() {
        let evt = derive_chatroom_event("group@chatroom", "wxid_yyy退出了群聊");
        assert!(evt.is_some());
        let evt = evt.expect("left event");
        assert_eq!(evt.kind, kinds::CHATROOM_MEMBER_LEFT);
        assert_eq!(evt.chat_id().unwrap_or_default(), "group@chatroom");
    }

    #[test]
    fn derive_chatroom_event_no_match_returns_none() {
        let evt = derive_chatroom_event("abc@chatroom", "这是一条普通文本消息");
        assert!(evt.is_none());
    }

    #[test]
    fn derive_chatroom_event_non_chatroom_is_none() {
        let evt = derive_chatroom_event("wxid_user", "wxid_x加入了群聊");
        assert!(evt.is_none());
    }
}
