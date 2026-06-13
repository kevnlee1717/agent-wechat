//! EventBus：broadcast 广播 + RingBuffer 回溯，提供 at-least-once 的基础能力。

use crate::events::Event;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct EventBus {
    sender: broadcast::Sender<Event>,
    ring: Arc<RwLock<VecDeque<Event>>>,
    ring_capacity: usize,
}

impl EventBus {
    pub fn new(broadcast_capacity: usize, ring_capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(broadcast_capacity);
        Self {
            sender,
            ring: Arc::new(RwLock::new(VecDeque::with_capacity(ring_capacity))),
            ring_capacity,
        }
    }

    /// 发布事件，同时写入 ring buffer 并广播给所有订阅者。
    pub fn publish(&self, event: Event) {
        if self.ring_capacity > 0 {
            let mut ring = self.ring.write();
            if ring.len() >= self.ring_capacity {
                let _ = ring.pop_front();
            }
            ring.push_back(event.clone());
        }
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// 返回 since 之后未消费的事件；since 为空表示从现在开始，不回放。
    /// 当 since 在 buffer 之外且比最早事件更早，返回 gap 提示。
    pub fn replay_since(&self, since: Option<&str>) -> Result<Vec<Event>, ReplayGap> {
        let ring = self.ring.read();
        let since = match since {
            None => return Ok(Vec::new()),
            Some(since) => since,
        };

        match ring.iter().position(|e| e.id.as_str() == since) {
            Some(idx) => Ok(ring.iter().skip(idx + 1).cloned().collect()),
            None => {
                if let Some(front) = ring.front() {
                    if since < front.id.as_str() {
                        return Err(ReplayGap {
                            earliest_in_buffer: front.id.clone(),
                        });
                    }
                }
                Ok(Vec::new())
            }
        }
    }
}

#[derive(Debug)]
pub struct ReplayGap {
    pub earliest_in_buffer: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(kind: &str) -> Event {
        Event::new(kind, json!({}))
    }

    #[tokio::test]
    async fn publish_and_subscribe_receives() {
        let bus = EventBus::new(16, 100);
        let mut rx = bus.subscribe();
        let e = ev("test.a");
        bus.publish(e.clone());
        let recv = rx.recv().await.unwrap();
        assert_eq!(recv.id, e.id);
    }

    #[test]
    fn replay_since_returns_newer_events() {
        let bus = EventBus::new(16, 100);
        let e1 = ev("test.a");
        bus.publish(e1.clone());
        let e2 = ev("test.b");
        bus.publish(e2.clone());
        let e3 = ev("test.c");
        bus.publish(e3.clone());
        let replayed = bus.replay_since(Some(&e1.id)).unwrap();
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].id, e2.id);
        assert_eq!(replayed[1].id, e3.id);
    }

    #[test]
    fn replay_since_none_returns_empty() {
        let bus = EventBus::new(16, 100);
        bus.publish(ev("a"));
        assert!(bus.replay_since(None).unwrap().is_empty());
    }

    #[test]
    fn replay_since_too_old_returns_gap() {
        let bus = EventBus::new(16, 3);
        bus.publish(ev("a"));
        bus.publish(ev("b"));
        bus.publish(ev("c"));
        bus.publish(ev("d"));
        let err = bus.replay_since(Some("evt_00000000000000000000000000")).unwrap_err();
        assert!(!err.earliest_in_buffer.is_empty());
    }

    #[test]
    fn ring_buffer_caps_at_capacity() {
        let bus = EventBus::new(16, 2);
        bus.publish(ev("a"));
        bus.publish(ev("b"));
        bus.publish(ev("c"));
        assert_eq!(bus.ring.read().len(), 2);
    }
}
