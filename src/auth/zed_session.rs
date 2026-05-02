//! Zed native-app login session store.

use std::{collections::HashMap, sync::Arc};

use chrono::{DateTime, Duration, Utc};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::auth::zed_callback::CallbackState;

pub const ZED_LOGIN_SESSION_TTL_SECS: i64 = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZedLoginSessionStatus {
    Waiting,
    Completed,
    Failed(String),
}

pub struct ZedLoginSession {
    pub name: String,
    pub private_key: String,
    pub port: u16,
    pub status: ZedLoginSessionStatus,
    pub created_at: DateTime<Utc>,
    pub callback_state: Arc<CallbackState>,
    pub server_handle: JoinHandle<()>,
}

impl ZedLoginSession {
    pub fn new(
        name: String,
        private_key: String,
        port: u16,
        callback_state: Arc<CallbackState>,
        server_handle: JoinHandle<()>,
    ) -> Self {
        Self {
            name,
            private_key,
            port,
            status: ZedLoginSessionStatus::Waiting,
            created_at: Utc::now(),
            callback_state,
            server_handle,
        }
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.created_at < now - Duration::seconds(ZED_LOGIN_SESSION_TTL_SECS)
    }
}

pub type ZedLoginSessionStore = Arc<Mutex<HashMap<String, ZedLoginSession>>>;

pub fn new_session_store() -> ZedLoginSessionStore {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn cleanup_expired_sessions(sessions: &mut HashMap<String, ZedLoginSession>) {
    let now = Utc::now();
    let mut expired_sessions = Vec::new();

    for (session_id, session) in sessions.iter() {
        if session.is_expired(now) {
            expired_sessions.push(session_id.clone());
        }
    }

    for session_id in expired_sessions {
        if let Some(expired_session) = sessions.remove(&session_id) {
            expired_session.server_handle.abort();
        }
    }
}
