//! 好友请求消息轮询器。

use crate::db::get_db;
use crate::events::pollers::Poller;
use crate::events::{bus::EventBus, kinds, Event};
use crate::ia::types::Message;
use crate::sessions::manager::get_session;
use crate::tools::wechat_db::{find_wechat_pid, list_account_dbs};
use crate::tools::wechat_keys::{extract_keys_async, get_stored_keys, store_keys};
use crate::tools::wechat_messages;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;

/// 好友请求 poller。
/// 游标是 fmessage chat 内的 local_id，避免漏发/重发。
pub struct FriendRequestPoller {
    pub last_seen_local_id: i64,
    pub interval_ms: u64,
    pub cold_start: bool,
}

#[derive(Debug, Clone)]
struct FriendRequest {
    from_wxid: String,
    from_name: String,
    alias: String,
    scene: String,
    content: String,
}

impl FriendRequestPoller {
    pub fn new(interval_ms: u64) -> Self {
        Self {
            last_seen_local_id: 0,
            interval_ms,
            cold_start: true,
        }
    }
}

#[async_trait]
impl Poller for FriendRequestPoller {
    fn name(&self) -> &'static str {
        "friend_request"
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

        if !keys.keys().any(|k| k.starts_with("message_") && k.ends_with(".db")) {
            return Ok(());
        }

        let mut messages = wechat_messages::list_messages(&logged_in_user, &keys, "fmessage", 50, 0);
        messages.sort_by_key(|m| m.local_id);

        let events = collect_friend_request_events(&messages, &mut self.last_seen_local_id, self.cold_start);
        if !self.cold_start {
            for event in events {
                bus.publish(event);
            }
        }

        if self.cold_start {
            self.cold_start = false;
        }

        Ok(())
    }
}

fn collect_friend_request_events(
    messages: &[Message],
    cursor: &mut i64,
    cold_start: bool,
) -> Vec<Event> {
    let mut events = Vec::new();
    let mut latest_local_id = *cursor;

    for msg in messages {
        if msg.msg_type != 37 {
            continue;
        }
        if msg.local_id <= *cursor {
            continue;
        }
        latest_local_id = latest_local_id.max(msg.local_id);
        if cold_start {
            continue;
        }

        let parsed = parse_friend_request_xml(&msg.content);
        events.push(Event::new(
            kinds::CONTACT_FRIEND_REQUEST,
            json!({
                "fromWxid": parsed.from_wxid,
                "fromName": parsed.from_name,
                "alias": parsed.alias,
                "scene": parsed.scene,
                "content": parsed.content,
            }),
        ));
    }

    *cursor = latest_local_id;
    events
}

fn parse_friend_request_xml(content: &str) -> FriendRequest {
    let snippet = extract_msg_tag(content);
    FriendRequest {
        from_wxid: extract_attr(&snippet, "fromusername"),
        from_name: extract_attr(&snippet, "fromnickname"),
        alias: extract_attr(&snippet, "alias"),
        scene: extract_attr(&snippet, "scene"),
        content: extract_attr(&snippet, "content"),
    }
}

fn extract_msg_tag(content: &str) -> String {
    if let Some(start) = content.find("<msg") {
        if let Some(end) = content[start..].find(">").map(|i| start + i + 1) {
            return content[start..end].to_string();
        }
    }
    content.to_string()
}

fn extract_attr(xml: &str, key: &str) -> String {
    let markers = [format!("{key}=\""), format!("{key}='")];
    for marker in markers {
        if let Some(start) = xml.find(&marker) {
            let body = &xml[start + marker.len()..];
            let quote = if marker.ends_with('\"') { '"' } else { '\'' };
            if let Some(end) = body.find(quote) {
                return body[..end].to_string();
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn parse_friend_request_xml_fields() {
        let xml = r#"<msg fromusername="wxid_from" fromnickname="张三" alias="zhangsan" scene="14" content="你好，我想加你" />"#;
        let parsed = parse_friend_request_xml(xml);
        assert_eq!(parsed.from_wxid, "wxid_from");
        assert_eq!(parsed.from_name, "张三");
        assert_eq!(parsed.alias, "zhangsan");
        assert_eq!(parsed.scene, "14");
        assert_eq!(parsed.content, "你好，我想加你");
    }

    #[test]
    fn parse_friend_request_xml_tolerant_to_missing() {
        let xml = r#"<msg fromusername="wxid_from" />"#;
        let parsed = parse_friend_request_xml(xml);
        assert_eq!(parsed.from_wxid, "wxid_from");
        assert_eq!(parsed.from_name, "");
        assert_eq!(parsed.alias, "");
        assert_eq!(parsed.scene, "");
        assert_eq!(parsed.content, "");
    }

    #[test]
    fn friend_request_cold_start_then_emit_only_new() {
        let mut cursor = 0;
        let messages = vec![
            Message {
                local_id: 1,
                server_id: 1,
                chat_id: "fmessage".to_string(),
                sender: None,
                sender_name: None,
                msg_type: 37,
                content: r#"<msg fromusername="wxid_a" fromnickname="A" alias="a" scene="14" content="hello" />"#.to_string(),
                timestamp: "2026-05-27T00:00:00Z".to_string(),
                is_mentioned: None,
                mentioned_wxids: None,
                is_self: None,
                reply: None,
            },
            Message {
                local_id: 2,
                server_id: 2,
                chat_id: "fmessage".to_string(),
                sender: None,
                sender_name: None,
                msg_type: 37,
                content: r#"<msg fromusername="wxid_b" fromnickname="B" alias="b" scene="14" content="world" />"#.to_string(),
                timestamp: "2026-05-27T00:00:01Z".to_string(),
                is_mentioned: None,
                mentioned_wxids: None,
                is_self: None,
                reply: None,
            },
        ];

        let events = collect_friend_request_events(&messages, &mut cursor, true);
        assert!(events.is_empty());
        assert_eq!(cursor, 2);

        let events = collect_friend_request_events(&messages, &mut cursor, false);
        assert!(events.is_empty());

        let messages = vec![
            Message {
                local_id: 3,
                server_id: 3,
                chat_id: "fmessage".to_string(),
                sender: None,
                sender_name: None,
                msg_type: 37,
                content: r#"<msg fromusername="wxid_c" fromnickname="C" alias="c" scene="14" content="new" />"#.to_string(),
                timestamp: "2026-05-27T00:00:02Z".to_string(),
                is_mentioned: None,
                mentioned_wxids: None,
                is_self: None,
                reply: None,
            },
        ];
        let events = collect_friend_request_events(&messages, &mut cursor, false);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, kinds::CONTACT_FRIEND_REQUEST);
        assert_eq!(
            events[0].data.get("fromWxid").and_then(Value::as_str),
            Some("wxid_c")
        );
        assert_eq!(cursor, 3);
    }
}
