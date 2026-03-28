use std::collections::HashMap;

use tokio::sync::RwLock;

/// Tracks sticky auth selection per execution session ID.
#[derive(Default)]
pub struct ExecutionSessionStore {
    selected_auth_by_session: RwLock<HashMap<String, String>>,
}

impl ExecutionSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get_selected_auth(&self, session_id: &str) -> Option<String> {
        let sessions = self.selected_auth_by_session.read().await;
        sessions.get(session_id).cloned()
    }

    pub async fn set_selected_auth(&self, session_id: String, selected_auth_id: String) {
        let mut sessions = self.selected_auth_by_session.write().await;
        sessions.insert(session_id, selected_auth_id);
    }
}
