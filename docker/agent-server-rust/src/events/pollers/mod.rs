//! 事件轮询器公共接口与辅助函数。

pub mod message;
pub mod friend_request;
pub mod contact_diff;
pub mod session_emitter;
pub mod media;

use crate::events::bus::EventBus;
use async_trait::async_trait;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time;

#[async_trait]
pub trait Poller: Send + 'static {
    fn name(&self) -> &'static str;
    fn interval(&self) -> Duration;
    async fn poll(&mut self, bus: &EventBus) -> Result<()>;
}

/// 启动 Poller 循环任务。
///
/// 失败只记录 warn，不 panic，避免单个 Poller 退出影响进程。
pub fn spawn<P: Poller>(poller: P, bus: Arc<EventBus>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let name = poller.name();
        let mut poller = poller;
        let mut ticker = time::interval(poller.interval());
        ticker.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            if let Err(e) = poller.poll(&bus).await {
                tracing::warn!(poller = name, error = %e, "poller failed");
            }
        }
    })
}
