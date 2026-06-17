use axum::{
    body::Bytes,
    extract::Path,
    http::{header, StatusCode},
    response::IntoResponse,
};
use std::path::PathBuf;

/// Serve decrypted media bytes from WECHAT_MEDIA_DIR.
/// Path is provided as a wildcard from `/api/media/{*path}`.
pub async fn get_media(Path(rel): Path<String>) -> impl IntoResponse {
    if rel.contains("..") {
        return StatusCode::FORBIDDEN.into_response();
    }

    let media_dir = std::env::var("WECHAT_MEDIA_DIR").unwrap_or_else(|_| "/wechat-media".into());
    let media_root = PathBuf::from(media_dir);
    let media_root = match media_root.canonicalize() {
        Ok(path) => path,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let media_path = media_root.join(&rel);
    let media_path = match media_path.canonicalize() {
        Ok(path) => path,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    if !media_path.starts_with(&media_root) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let bytes = match tokio::fs::read(&media_path).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let ext = media_path.extension().and_then(|x| x.to_str()).unwrap_or("");
    let mime = match ext.to_ascii_lowercase().as_str() {
        "jpeg" | "jpg" => "image/jpeg",
        "png" => "image/png",
        "mp4" => "video/mp4",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    };

    ([(header::CONTENT_TYPE, mime)], Bytes::from(bytes)).into_response()
}
