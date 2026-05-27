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
        }
    }
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
