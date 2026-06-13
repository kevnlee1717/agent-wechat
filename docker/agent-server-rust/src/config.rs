use serde::Deserialize;

/// websocket 推送相关配置。
#[derive(Deserialize, Clone)]
pub struct WsConfig {
    #[serde(default = "default_ring_buffer_size")]
    pub ring_buffer_size: usize,
    #[serde(default = "default_broadcast_capacity")]
    pub broadcast_capacity: usize,
    #[serde(default = "default_heartbeat_interval_secs")]
    pub heartbeat_interval_secs: u64,
}

/// Poller 相关配置。
#[derive(Deserialize, Clone)]
pub struct PollersConfig {
    #[serde(default = "default_message_interval_ms")]
    pub message_interval_ms: u64,
    #[serde(default = "default_friend_request_interval_ms")]
    pub friend_request_interval_ms: u64,
    #[serde(default = "default_contact_diff_interval_secs")]
    pub contact_diff_interval_secs: u64,
    #[serde(default = "default_media_interval_ms")]
    pub media_interval_ms: Option<u64>,
    #[serde(default = "default_image_fetch_original")]
    pub image_fetch_original: bool,
    #[serde(default = "default_image_fetch_timeout_ms")]
    pub image_fetch_timeout_ms: u64,
}

impl Default for WsConfig {
    fn default() -> Self {
        Self {
            ring_buffer_size: default_ring_buffer_size(),
            broadcast_capacity: default_broadcast_capacity(),
            heartbeat_interval_secs: default_heartbeat_interval_secs(),
        }
    }
}

impl Default for PollersConfig {
    fn default() -> Self {
        Self {
            message_interval_ms: default_message_interval_ms(),
            friend_request_interval_ms: default_friend_request_interval_ms(),
            contact_diff_interval_secs: default_contact_diff_interval_secs(),
            media_interval_ms: default_media_interval_ms(),
            image_fetch_original: default_image_fetch_original(),
            image_fetch_timeout_ms: default_image_fetch_timeout_ms(),
        }
    }
}

impl WsConfig {
    pub fn from_env() -> Self {
        Self {
            ring_buffer_size: std::env::var("YOYO_WS_RING_BUFFER_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_ring_buffer_size),
            broadcast_capacity: std::env::var("YOYO_WS_BROADCAST_CAPACITY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_broadcast_capacity),
            heartbeat_interval_secs: std::env::var("YOYO_WS_HEARTBEAT_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_heartbeat_interval_secs),
        }
    }
}

impl PollersConfig {
    pub fn from_env() -> Self {
        Self {
            message_interval_ms: std::env::var("YOYO_WS_MESSAGE_INTERVAL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_message_interval_ms),
            friend_request_interval_ms: std::env::var("YOYO_WS_FRIEND_REQUEST_INTERVAL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_friend_request_interval_ms),
            contact_diff_interval_secs: std::env::var("YOYO_WS_CONTACT_DIFF_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(default_contact_diff_interval_secs),
            media_interval_ms: std::env::var("YOYO_WS_MEDIA_INTERVAL_MS")
                .ok()
                .and_then(|s| s.parse().ok()),
            image_fetch_original: std::env::var("YOYO_WS_IMAGE_FETCH_ORIGINAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(true),
            image_fetch_timeout_ms: std::env::var("YOYO_WS_IMAGE_FETCH_TIMEOUT_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5000),
        }
    }
}

/// Enable receive-only mode.
///
/// When enabled, health monitor and auth status use cheap process/db signals
/// and avoid running accessibility/screenshot loops in steady state.
pub fn receive_only() -> bool {
    parse_bool_env_var(std::env::var("AGENT_WECHAT_RECEIVE_ONLY").ok())
}

/// Optional periodic a11y probe interval for RECEIVE_ONLY mode.
///
/// 0 means disabled.
pub fn receive_only_a11y_probe_secs() -> u64 {
    std::env::var("AGENT_WECHAT_RO_A11Y_PROBE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn parse_bool_env_var(value: Option<String>) -> bool {
    value
        .map(|value| matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ))
        .unwrap_or(false)
}

fn default_ring_buffer_size() -> usize {
    10_000
}

fn default_broadcast_capacity() -> usize {
    1024
}

fn default_heartbeat_interval_secs() -> u64 {
    30
}

fn default_message_interval_ms() -> u64 {
    200
}

fn default_friend_request_interval_ms() -> u64 {
    500
}

fn default_contact_diff_interval_secs() -> u64 {
    60
}

fn default_media_interval_ms() -> Option<u64> {
    Some(2000)
}

fn default_image_fetch_original() -> bool {
    true
}

fn default_image_fetch_timeout_ms() -> u64 {
    5000
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_MUTEX
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap()
    }

    #[test]
    fn test_receive_only_env_var_parsing() {
        let _guard = env_lock();
        let key = "AGENT_WECHAT_RECEIVE_ONLY";
        let prev: Option<OsString> = std::env::var_os(key);

        std::env::remove_var(key);
        assert!(!super::receive_only());

        std::env::set_var(key, "1");
        assert!(super::receive_only());
        std::env::set_var(key, "true");
        assert!(super::receive_only());
        std::env::set_var(key, "on");
        assert!(super::receive_only());
        std::env::set_var(key, "false");
        assert!(!super::receive_only());
        std::env::set_var(key, "0");
        assert!(!super::receive_only());

        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn test_receive_only_probe_secs_default_off() {
        let _guard = env_lock();
        let key = "AGENT_WECHAT_RO_A11Y_PROBE_SECS";
        let prev: Option<OsString> = std::env::var_os(key);

        std::env::remove_var(key);
        assert_eq!(super::receive_only_a11y_probe_secs(), 0);

        std::env::set_var(key, "30");
        assert_eq!(super::receive_only_a11y_probe_secs(), 30);

        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}
