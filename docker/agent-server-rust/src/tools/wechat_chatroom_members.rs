use std::collections::HashMap;

use crate::ia::types::ChatroomMember;
use crate::tools::wechat_db::{get_db_path, query_wechat_db_params};

const CONTACT_DB_NAME: &str = "contact.db";

/// Decrypt contact.db and run the tested join to return the full chatroom roster.
pub fn list_chatroom_members(
    account_dir: &str,
    keys: &HashMap<String, String>,
    chatroom_id: &str,
) -> Vec<ChatroomMember> {
    let key = match keys.get(CONTACT_DB_NAME) {
        Some(k) => k,
        None => return Vec::new(),
    };

    let db_path = get_db_path(account_dir, CONTACT_DB_NAME);
    let sql = "SELECT n2.username AS wxid, c.nick_name AS nickname \
               FROM chatroom_member m \
               JOIN name2id rn ON rn.rowid = m.room_id \
               JOIN name2id n2 ON n2.rowid = m.member_id \
               LEFT JOIN contact c ON c.username = n2.username \
               WHERE rn.username = ?1;";
    let rows = query_wechat_db_params(&db_path, key, sql, &[&chatroom_id]);

    rows.into_iter()
        .filter_map(|r| {
            let wxid = r.get("wxid").and_then(|v| v.as_str())?.to_string();
            if wxid.is_empty() {
                return None;
            }
            let nickname = r
                .get("nickname")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ChatroomMember { wxid, nickname })
        })
        .collect()
}
