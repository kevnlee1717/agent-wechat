use axum::{extract::Query, Json};
use serde::{Deserialize, Serialize};

use crate::tools::a11y::{get_a11y_desktop, tree_to_aria};
use crate::tools::exec::ExecOptions;
use crate::tools::screenshot::capture_screenshot;

#[derive(Serialize)]
pub struct ScreenshotResponse {
    base64: String,
}

pub async fn screenshot() -> Json<ScreenshotResponse> {
    let b64 = capture_screenshot(&ExecOptions::default())
        .await
        .unwrap_or_default();
    Json(ScreenshotResponse { base64: b64 })
}

#[derive(Deserialize)]
pub struct A11yParams {
    #[serde(default = "default_format")]
    format: String,
}

fn default_format() -> String {
    "json".to_string()
}

#[derive(Serialize)]
pub struct A11yResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    tree: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aria: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub async fn a11y(Query(params): Query<A11yParams>) -> Json<A11yResponse> {
    let result = get_a11y_desktop(&ExecOptions::default()).await;

    match result {
        Ok(tree) => {
            if params.format == "aria" {
                Json(A11yResponse {
                    tree: None,
                    aria: Some(tree_to_aria(&tree, 0)),
                    error: None,
                })
            } else {
                // Serialize tree without parent refs
                let json = serde_json::to_value(&tree).ok();
                Json(A11yResponse {
                    tree: json,
                    aria: None,
                    error: None,
                })
            }
        }
        Err(e) => Json(A11yResponse {
            tree: None,
            aria: None,
            error: Some(e),
        }),
    }
}
