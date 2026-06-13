use axum::{extract::Path, Json};
use serde::Deserialize;

use crate::ia::types::Session;
use crate::sessions::manager;

pub async fn list_sessions() -> Json<Vec<Session>> {
    Json(manager::list_sessions())
}

#[derive(Deserialize)]
pub struct CreateParams {
    name: String,
}

pub async fn create_session(Json(input): Json<CreateParams>) -> Json<serde_json::Value> {
    match manager::create_session(&input.name).await {
        Ok(session) => Json(serde_json::to_value(session).unwrap()),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

pub async fn get_session(Path(id): Path<String>) -> Json<Option<Session>> {
    Json(manager::get_session(&id))
}

pub async fn start_session(Path(id): Path<String>) -> Json<serde_json::Value> {
    match manager::start_session(&id).await {
        Ok(session) => Json(serde_json::to_value(session).unwrap()),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

pub async fn stop_session(Path(id): Path<String>) -> Json<serde_json::Value> {
    match manager::stop_session(&id).await {
        Ok(session) => Json(serde_json::to_value(session).unwrap()),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}

pub async fn delete_session(Path(id): Path<String>) -> Json<serde_json::Value> {
    match manager::delete_session(&id).await {
        Ok(()) => Json(serde_json::json!({ "success": true })),
        Err(e) => Json(serde_json::json!({ "error": e })),
    }
}
