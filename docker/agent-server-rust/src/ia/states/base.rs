use crate::ia::selectors::query_selector;
use crate::ia::types::{A11yNode, Bounds};

/// Window control button bounds extracted from a frame.
pub struct WindowControlBounds {
    pub close_button_bounds: Option<Bounds>,
    pub minimize_button_bounds: Option<Bounds>,
    pub maximize_button_bounds: Option<Bounds>,
}

/// Extract window control button bounds from a frame node.
pub fn extract_window_control_bounds(frame: Option<&A11yNode>) -> WindowControlBounds {
    let frame = match frame {
        Some(f) => f,
        None => {
            return WindowControlBounds {
                close_button_bounds: None,
                minimize_button_bounds: None,
                maximize_button_bounds: None,
            }
        }
    };

    let close_btn = query_selector(frame, r#"tool-bar push-button[name="Disable"]"#);
    let minimize_btn = query_selector(frame, r#"tool-bar push-button[name="Minimize"]"#);
    let maximize_btn = query_selector(frame, r#"tool-bar push-button[name="Maximize"]"#);

    WindowControlBounds {
        close_button_bounds: close_btn.and_then(|n| n.bounds.clone()),
        minimize_button_bounds: minimize_btn.and_then(|n| n.bounds.clone()),
        maximize_button_bounds: maximize_btn.and_then(|n| n.bounds.clone()),
    }
}
