use axum::{extract::Path, Json};

use crate::db::get_db;
use crate::ia::types::ChatroomMember;
use crate::sessions::manager::get_session;
use crate::tools::wechat_chatroom_members::list_chatroom_members;
use crate::tools::wechat_db::find_wechat_pid;
use crate::tools::wechat_keys::{extract_keys_async, get_stored_keys, store_keys};

const CONTACT_DB_NAME: &str = "contact.db";

pub async fn list_members(Path(id): Path<String>) -> Json<Vec<ChatroomMember>> {
    let session = match get_session("default") {
        Some(s) => s,
        None => return Json(Vec::new()),
    };
    let logged_in_user = match &session.logged_in_user {
        Some(u) => u.clone(),
        None => return Json(Vec::new()),
    };

    let mut keys = {
        let db = get_db();
        get_stored_keys(&db, &session.id, &logged_in_user)
    };

    if !keys.contains_key(CONTACT_DB_NAME) {
        if let Some(pid) = find_wechat_pid() {
            let extracted = extract_keys_async(pid).await;
            if !extracted.is_empty() {
                let db = get_db();
                store_keys(&db, &session.id, &logged_in_user, &extracted);
                keys = get_stored_keys(&db, &session.id, &logged_in_user);
            }
        }
    }

    if !keys.contains_key(CONTACT_DB_NAME) {
        return Json(Vec::new());
    }

    Json(list_chatroom_members(&logged_in_user, &keys, &id))
}
