#![allow(dead_code)]

mod context;
mod db;
mod effects;
mod execution;
mod ia;
mod config;
mod events;
mod tools;
mod plans;
mod router;
mod sessions;

use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let port: u16 = std::env::var("AGENT_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6174);
    let host = std::env::var("AGENT_HOST").unwrap_or_else(|_| "0.0.0.0".into());

    // Log environment
    tracing::info!("Environment:");
    tracing::info!("  DISPLAY: {:?}", std::env::var("DISPLAY").ok());
    tracing::info!(
        "  DBUS_SESSION_BUS_ADDRESS: {:?}",
        std::env::var("DBUS_SESSION_BUS_ADDRESS").ok()
    );

    // Initialize auth token (panics if no token found)
    router::auth::init_token();
    tracing::info!("Auth token loaded");

    // Initialize database
    tracing::info!("Initializing database...");
    db::init_db().expect("Failed to initialize database");

    // Initialize sessions
    tracing::info!("Initializing sessions...");
    sessions::manager::initialize_sessions()
        .await
        .expect("Failed to initialize sessions");

    // Initialize event bus and pollers
    let ws_config = config::WsConfig::from_env();
    let pollers_config = config::PollersConfig::from_env();
    let bus = std::sync::Arc::new(
        crate::events::bus::EventBus::new(ws_config.broadcast_capacity, ws_config.ring_buffer_size),
    );
    crate::events::set_global_bus(bus.clone());

    crate::events::pollers::spawn(
        crate::events::pollers::message::MessagePoller::new(pollers_config.message_interval_ms),
        bus.clone(),
    );
    crate::events::pollers::spawn(
        crate::events::pollers::friend_request::FriendRequestPoller::new(
            pollers_config.friend_request_interval_ms,
        ),
        bus.clone(),
    );
    crate::events::pollers::spawn(
        crate::events::pollers::contact_diff::ContactDiffPoller::new(
            pollers_config.contact_diff_interval_secs,
        ),
        bus.clone(),
    );
    let media_dir = std::env::var("WECHAT_MEDIA_DIR").unwrap_or_else(|_| "/wechat-media".into());
    let media_path = std::path::PathBuf::from(media_dir);
    if let Err(e) = std::fs::create_dir_all(&media_path) {
        tracing::warn!(
            error = %e,
            "WECHAT_MEDIA_DIR 创建失败，MediaPoller 仍会启动",
        );
    }
    crate::events::pollers::spawn(
        crate::events::pollers::media::MediaPoller::new(
            pollers_config.media_interval_ms.unwrap_or(2000),
            media_path,
            pollers_config.image_fetch_original,
            pollers_config.image_fetch_timeout_ms,
        ),
        bus.clone(),
    );

    // Start background health monitor
    sessions::health_monitor::spawn_health_monitor();
    tracing::info!("WeChat health monitor started");

    // Build router
    let app = router::build_router();

    let addr: SocketAddr = format!("{host}:{port}").parse().expect("Invalid address");
    tracing::info!("agent-server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl+c");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutting down...");
}
