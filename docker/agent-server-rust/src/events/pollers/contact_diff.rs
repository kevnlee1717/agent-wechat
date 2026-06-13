//! 联系人差量 poller。
//! 对比 contact.db snapshot，检测新增/消失联系人。

use crate::db::get_db;
use crate::events::pollers::Poller;
use crate::events::{bus::EventBus, kinds, Event};
use crate::sessions::manager::get_session;
use crate::tools::wechat_db::{get_db_path, query_wechat_db};
use crate::tools::wechat_keys::get_stored_keys;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

pub struct ContactDiffPoller {
    last_snapshot: HashMap<String, ContactInfo>,
    pub interval_secs: u64,
    pub cold_start: bool,
}

#[derive(Debug, Clone, Default)]
struct ContactInfo {
    nick_name: String,
    remark: String,
}

impl ContactDiffPoller {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            last_snapshot: HashMap::new(),
            interval_secs,
            cold_start: true,
        }
    }
}

#[async_trait]
impl Poller for ContactDiffPoller {
    fn name(&self) -> &'static str {
        "contact_diff"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_secs)
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

        let db = get_db();
        let keys = get_stored_keys(&db, &session.id, &logged_in_user);
        let contact_key = match keys.get("contact.db") {
            Some(k) => k.clone(),
            None => return Ok(()),
        };

        let db_path = get_db_path(&logged_in_user, "contact.db");
        let rows = query_wechat_db(
            &db_path,
            &contact_key,
            "SELECT username, nick_name, remark FROM contact WHERE local_type = 1 AND delete_flag = 0",
        );
        let mut snapshot = HashMap::new();
        for row in rows {
            let username = row.get("username").and_then(|v| v.as_str()).unwrap_or_default();
            if username.is_empty() {
                continue;
            }
            snapshot.insert(
                username.to_string(),
                ContactInfo {
                    nick_name: row.get("nick_name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    remark: row.get("remark").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                },
            );
        }

        if self.cold_start {
            self.last_snapshot = snapshot;
            self.cold_start = false;
            return Ok(());
        }

        emit_diff_events(&snapshot, &self.last_snapshot, bus);
        self.last_snapshot = snapshot;
        Ok(())
    }
}

fn emit_diff_events(
    current: &HashMap<String, ContactInfo>,
    previous: &HashMap<String, ContactInfo>,
    bus: &EventBus,
) {
    for (wxid, info) in current {
        if !previous.contains_key(wxid) {
            bus.publish(Event::new(
                kinds::CONTACT_ADDED,
                json!({
                    "wxid": wxid,
                    "nickName": info.nick_name.clone(),
                    "remark": info.remark.clone(),
                }),
            ));
        }
    }

    for (wxid, _) in previous {
        if !current.contains_key(wxid) {
            bus.publish(Event::new(
                kinds::CONTACT_REMOVED,
                json!({
                    "wxid": wxid,
                }),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn diff_snapshot_detects_added_and_removed() {
        let mut previous = HashMap::new();
        previous.insert(
            "wxid_a".to_string(),
            ContactInfo {
                nick_name: "A".to_string(),
                remark: "".to_string(),
            },
        );
        previous.insert(
            "wxid_b".to_string(),
            ContactInfo {
                nick_name: "B".to_string(),
                remark: "".to_string(),
            },
        );

        let mut current = HashMap::new();
        current.insert(
            "wxid_b".to_string(),
            ContactInfo {
                nick_name: "B2".to_string(),
                remark: "new".to_string(),
            },
        );
        current.insert(
            "wxid_c".to_string(),
            ContactInfo {
                nick_name: "C".to_string(),
                remark: "".to_string(),
            },
        );

        let bus = EventBus::new(16, 10);
        let mut receiver = bus.subscribe();
        emit_diff_events(&current, &previous, &bus);
        let e1 = receiver.recv().await.unwrap();
        let e2 = receiver.recv().await.unwrap();
        let kinds_vec = vec![e1.kind.clone(), e2.kind.clone()];
        assert!(kinds_vec.contains(&kinds::CONTACT_ADDED.to_string()));
        assert!(kinds_vec.contains(&kinds::CONTACT_REMOVED.to_string()));
        let mut found = Vec::new();
        for evt in [e1, e2] {
            if let Some(wxid) = evt.data.get("wxid").and_then(Value::as_str) {
                found.push(wxid.to_string());
            }
        }
        assert!(found.contains(&"wxid_c".to_string()));
        assert!(found.contains(&"wxid_a".to_string()));
    }
}
