//! 事件总线相关公共类型定义。
//! 上游 WS 订阅方消费的都是同一个事件信封。

pub mod bus;
pub mod id;
pub mod pollers;

use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

/// 统一事件信封。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// schema 版本号，当前固定 1
    pub v: u8,
    /// 全局唯一事件 ID（ULID）
    pub id: String,
    /// 事件产生时间戳（ISO 8601 UTC）
    pub ts: String,
    /// 事件类型
    #[serde(rename = "type")]
    pub kind: String,
    /// 事件载荷
    pub data: serde_json::Value,
}

impl Event {
    /// 创建新事件实例。
    pub fn new(kind: &str, data: serde_json::Value) -> Self {
        Self {
            v: 1,
            id: id::next_event_id(),
            ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            kind: kind.to_string(),
            data,
        }
    }

    /// 从 data 中读取 chatId，用于 chats 过滤。
    pub fn chat_id(&self) -> Option<&str> {
        self.data.get("chatId").and_then(|v| v.as_str())
    }
}

static GLOBAL_BUS: OnceLock<Arc<crate::events::bus::EventBus>> = OnceLock::new();

/// 设置全局 EventBus，供无 State 路由读取。
pub fn set_global_bus(bus: Arc<crate::events::bus::EventBus>) {
    let _ = GLOBAL_BUS.set(bus);
}

/// 读取全局 EventBus。
pub fn get_global_bus() -> Option<Arc<crate::events::bus::EventBus>> {
    GLOBAL_BUS.get().cloned()
}

/// 事件 type 常量（统一由单一入口使用）。
pub mod kinds {
    pub const MESSAGE_NEW: &str = "message.new";
    pub const SESSION_LOGIN: &str = "session.login";
    pub const SESSION_LOGOUT: &str = "session.logout";
    pub const SESSION_QR_REFRESH: &str = "session.qr_refresh";
    pub const SESSION_SCAN: &str = "session.scan";
    pub const CONTACT_FRIEND_REQUEST: &str = "contact.friend_request";
    pub const CONTACT_ADDED: &str = "contact.added";
    pub const CONTACT_REMOVED: &str = "contact.removed";
    pub const CHATROOM_MEMBER_JOINED: &str = "chatroom.member_joined";
    pub const CHATROOM_MEMBER_LEFT: &str = "chatroom.member_left";
}
