use crate::ia::types::{AppState, Session};
use rusqlite::{params, Connection};

/// Context manages the persistent FSM state.
pub struct Context {
    pub session_id: String,
    pub session: Session,
    pub state: AppState,
}

impl Context {
    pub fn new(session: Session) -> Self {
        let session_id = session.id.clone();
        Self {
            session_id,
            session,
            state: AppState::default(),
        }
    }

    /// Load context from database.
    pub fn load(&mut self, conn: &Connection) {
        let result: Option<String> = conn
            .query_row(
                "SELECT app_state FROM context WHERE session_id = ?1",
                params![self.session_id],
                |row| row.get(0),
            )
            .ok();

        if let Some(json) = result {
            if let Ok(parsed) = serde_json::from_str::<AppState>(&json) {
                self.state = parsed;
            }
        }
    }

    /// Save context to database.
    pub fn save(&self, conn: &Connection) {
        if let Ok(json) = serde_json::to_string(&self.state) {
            conn.execute(
                "INSERT INTO context (session_id, app_state, updated_at)
                 VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(session_id) DO UPDATE SET
                   app_state = excluded.app_state,
                   updated_at = datetime('now')",
                params![self.session_id, json],
            )
            .ok();
        }
    }
}

/// Create a Context for a session, loading persisted state.
pub fn create_context(session: Session, conn: &Connection) -> Context {
    let mut ctx = Context::new(session);
    ctx.load(conn);
    ctx
}
