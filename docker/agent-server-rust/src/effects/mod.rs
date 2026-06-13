use crate::ia::types::{AppState, Effect};

/// Collect effects from all watchers.
///
/// Currently empty — all login emissions are handled by the login plan
/// to ensure proper sequencing.
pub fn collect_effects(_prev: &AppState, _next: &AppState) -> Vec<Effect> {
    Vec::new()
}
