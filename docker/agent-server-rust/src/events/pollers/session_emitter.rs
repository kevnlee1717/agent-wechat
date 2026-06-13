//! session 生命周期事件 helper。
//! 不是 Poller，是在登录/登出/FSM hook 中被动调用的 emit 点。

use crate::events::{bus::EventBus, kinds, Event};
use serde_json::json;

pub fn emit_login(bus: &EventBus, wxid: &str, session_id: &str) {
    bus.publish(Event::new(
        kinds::SESSION_LOGIN,
        json!({
            "wxid": wxid,
            "sessionName": session_id,
        }),
    ));
}

pub fn emit_logout(bus: &EventBus, wxid: &str, reason: &str) {
    bus.publish(Event::new(
        kinds::SESSION_LOGOUT,
        json!({
            "wxid": wxid,
            "reason": reason,
        }),
    ));
}

pub fn emit_qr_refresh(bus: &EventBus, qr_payload: &str, expires_at: &str) {
    bus.publish(Event::new(
        kinds::SESSION_QR_REFRESH,
        json!({
            "qrPayload": qr_payload,
            "expiresAt": expires_at,
        }),
    ));
}

pub fn emit_scan(bus: &EventBus, stage: &str, wxid: Option<&str>) {
    let mut payload = json!({
        "stage": stage,
    });
    if let Some(v) = wxid {
        payload["wxid"] = serde_json::Value::String(v.to_string());
    }
    bus.publish(Event::new(kinds::SESSION_SCAN, payload));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn emit_login_event_has_expected_payload() {
        let bus = crate::events::bus::EventBus::new(16, 100);
        let mut rx = bus.subscribe();

        emit_login(&bus, "wxid_user", "default");
        let evt = rx.recv().await.unwrap();
        assert_eq!(evt.kind, kinds::SESSION_LOGIN);
        assert_eq!(evt.data.get("wxid").and_then(Value::as_str), Some("wxid_user"));
        assert_eq!(evt.data.get("sessionName").and_then(Value::as_str), Some("default"));
    }

    #[tokio::test]
    async fn emit_logout_event_has_expected_payload() {
        let bus = crate::events::bus::EventBus::new(16, 100);
        let mut rx = bus.subscribe();

        emit_logout(&bus, "wxid_user", "user_logout");
        let evt = rx.recv().await.unwrap();
        assert_eq!(evt.kind, kinds::SESSION_LOGOUT);
        assert_eq!(evt.data.get("reason").and_then(Value::as_str), Some("user_logout"));
    }

    #[tokio::test]
    async fn emit_scan_event_skips_empty_wxid() {
        let bus = crate::events::bus::EventBus::new(16, 100);
        let mut rx = bus.subscribe();

        emit_scan(&bus, "confirmed", None);
        let evt = rx.recv().await.unwrap();
        assert_eq!(evt.kind, kinds::SESSION_SCAN);
        assert_eq!(evt.data.get("stage").and_then(Value::as_str), Some("confirmed"));
        assert!(evt.data.get("wxid").is_none());
    }
}
