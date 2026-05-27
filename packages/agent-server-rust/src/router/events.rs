use axum::{
    extract::{
        ws::{close_code, CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Query,
    },
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use tokio::time::{interval, Duration};
use tokio::sync::broadcast;

use crate::events::{get_global_bus, Event};
use crate::router::auth::get_token;

#[derive(Deserialize)]
pub struct EventsWsQuery {
    token: Option<String>,
    topics: Option<String>,
    chats: Option<String>,
    since: Option<String>,
}

pub async fn events_ws(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    Query(params): Query<EventsWsQuery>,
) -> impl IntoResponse {
    let authorized = is_authorized(&headers, params.token.as_deref());
    let topics = parse_topics(params.topics);
    let chats = parse_chats(params.chats);
    let since = params.since.filter(|s| !s.is_empty());

    ws.on_upgrade(move |socket| handle_events_ws(socket, authorized, topics, chats, since))
}

async fn handle_events_ws(
    mut socket: WebSocket,
    authorized: bool,
    topics: Vec<String>,
    chats: Vec<String>,
    since: Option<String>,
) {
    if !authorized {
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code: close_code::POLICY,
                reason: "unauthorized".into(),
            })))
            .await;
        return;
    }

    let bus = match get_global_bus() {
        Some(bus) => bus,
        None => {
            let _ = socket
                .send(Message::Close(Some(CloseFrame {
                    code: close_code::ERROR,
                    reason: "event bus unavailable".into(),
                })))
                .await;
            return;
        }
    };

    let mut receiver = bus.subscribe();
    let replayed = bus.replay_since(since.as_deref());
    match replayed {
        Ok(replayed) => {
            let mut replayed_count = 0u64;
            for event in replayed {
                if matches_filter(&event, &topics, &chats) {
                    if send_event(&mut socket, &event).await.is_err() {
                        return;
                    }
                    replayed_count += 1;
                }
            }
            let _ = send_control(
                &mut socket,
                "replay.done",
                json!({"count": replayed_count, "since": since.clone().unwrap_or_default()}),
            )
            .await;
        }
        Err(gap) => {
            let _ = send_control(
                &mut socket,
                "replay.gap",
                json!({"reason": "gap", "earliest": gap.earliest_in_buffer, "since": since}),
            )
            .await;
            let _ = socket
                .send(Message::Close(Some(CloseFrame {
                    code: close_code::ERROR,
                    reason: "replay gap".into(),
                })))
                .await;
            return;
        }
    }

    let mut ticker = interval(Duration::from_secs(30));
    let mut missed_pongs = 0u8;

    loop {
        tokio::select! {
            recv_result = receiver.recv() => {
                match recv_result {
                    Ok(event) => {
                        if matches_filter(&event, &topics, &chats) {
                            if send_event(&mut socket, &event).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(error) => {
                        match error {
                            broadcast::error::RecvError::Lagged(lagged) => {
                                let _ = send_control(
                                    &mut socket,
                                    "replay.gap",
                                    json!({"reason": "lagged", "lagged": lagged}),
                                ).await;
                                let _ = socket.send(Message::Close(Some(CloseFrame {
                                    code: close_code::AGAIN,
                                    reason: format!("lagged by {lagged} messages").into(),
                                }))).await;
                                break;
                            }
                            broadcast::error::RecvError::Closed => break,
                        }
                    }
                }
            }
            _ = ticker.tick() => {
                missed_pongs += 1;
                if missed_pongs >= 2 {
                    let _ = socket.send(Message::Close(Some(CloseFrame {
                        code: close_code::AWAY,
                        reason: "missed pong".into(),
                    }))).await;
                    break;
                }
                if socket.send(Message::Ping(vec![].into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Pong(_))) => {
                        missed_pongs = 0;
                    }
                    Some(Ok(_)) => {}
                    _ => break,
                }
            }
        }
    }
}

fn is_authorized(headers: &HeaderMap, token: Option<&str>) -> bool {
    let expected = get_token();
    if let Some(token) = token {
        if token == expected {
            return true;
        }
    }

    extract_bearer(headers.get(header::AUTHORIZATION))
        .is_some_and(|v| v == expected)
}

fn parse_topics(value: Option<String>) -> Vec<String> {
    let topics = value
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<String>>()
        })
        .unwrap_or_else(|| vec!["*".to_string()]);
    if topics.is_empty() {
        vec!["*".to_string()]
    } else {
        topics
    }
}

fn parse_chats(value: Option<String>) -> Vec<String> {
    value
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != "*")
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn matches_filter(event: &Event, topics: &[String], chats: &[String]) -> bool {
    if !topic_matches(&event.kind, topics) {
        return false;
    }

    if chats.is_empty() || event.chat_id().is_none() {
        return true;
    }

    let chat_id = match event.chat_id() {
        Some(chat_id) => chat_id,
        None => return true,
    };
    chats.iter().any(|chat| chat == chat_id)
}

fn topic_matches(event_type: &str, topics: &[String]) -> bool {
    if topics.is_empty() {
        return true;
    }

    topics.iter().any(|topic| {
        if topic == "*" {
            return true;
        }
        if let Some(prefix) = topic.strip_suffix(".*") {
            return event_type.starts_with(prefix);
        }
        event_type == topic
    })
}

fn extract_bearer(header: Option<&HeaderValue>) -> Option<&str> {
    header
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
}

async fn send_event(socket: &mut WebSocket, event: &Event) -> Result<(), ()> {
    let payload = serde_json::to_string(event).map_err(|_| ())?;
    socket.send(Message::Text(payload.into())).await.map_err(|_| ())
}

async fn send_control(socket: &mut WebSocket, event_type: &str, data: serde_json::Value) {
    let payload = serde_json::json!({
        "type": event_type,
        "data": data,
    });
    if let Ok(text) = serde_json::to_string(&payload) {
        let _ = socket.send(Message::Text(text.into())).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_with_kind(kind: &str, chat_id: Option<&str>) -> Event {
        let mut data = json!({});
        if let Some(chat_id) = chat_id {
            data["chatId"] = serde_json::Value::String(chat_id.to_string());
        }
        Event {
            v: 1,
            id: "test".to_string(),
            ts: "2026-01-01T00:00:00Z".to_string(),
            kind: kind.to_string(),
            data,
        }
    }

    #[test]
    fn topic_matches_all() {
        let topics = vec!["*".to_string()];
        assert!(topic_matches("message.new", &topics));
        assert!(topic_matches("session.login", &topics));
    }

    #[test]
    fn topic_matches_prefix_and_exact() {
        let topics = vec!["message.*".to_string(), "session.login".to_string()];
        assert!(topic_matches("message.new", &topics));
        assert!(topic_matches("session.login", &topics));
        assert!(!topic_matches("session.logout", &topics));
    }

    #[test]
    fn matches_uses_topic_and_chats_filters() {
        let topics = vec!["message.*".to_string()];
        let chats = vec!["chat_a".to_string(), "chat_b".to_string()];
        assert!(matches_filter(&event_with_kind("message.new", Some("chat_a")), &topics, &chats));
        assert!(!matches_filter(&event_with_kind("message.new", Some("chat_z")), &topics, &chats));
        assert!(matches_filter(&event_with_kind("session.login", None), &vec!["*".to_string()], &chats));
        assert!(!matches_filter(&event_with_kind("session.login", Some("chat_a")), &topics, &chats));
    }

    #[test]
    fn extract_bearer_parses_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc123"),
        );
        assert_eq!(extract_bearer(headers.get(axum::http::header::AUTHORIZATION)), Some("abc123"));
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Basic abc123"),
        );
        assert_eq!(extract_bearer(headers.get(axum::http::header::AUTHORIZATION)), None);
        assert_eq!(extract_bearer(None), None);
    }
}
