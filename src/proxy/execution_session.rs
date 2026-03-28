use std::time::Duration;

use moka::{policy::EvictionPolicy, sync::Cache};

const DEFAULT_EXECUTION_SESSION_MAX_CAPACITY: u64 = 10_000;
const DEFAULT_EXECUTION_SESSION_TTL: Duration = Duration::from_secs(60 * 60);

/// Tracks sticky auth selection per execution session ID.
pub struct ExecutionSessionStore {
    selected_auth_by_session: Cache<String, String>,
}

impl ExecutionSessionStore {
    pub fn new() -> Self {
        Self::with_limits(
            DEFAULT_EXECUTION_SESSION_MAX_CAPACITY,
            DEFAULT_EXECUTION_SESSION_TTL,
        )
    }

    fn with_limits(max_capacity: u64, ttl: Duration) -> Self {
        Self {
            selected_auth_by_session: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(ttl)
                .eviction_policy(EvictionPolicy::lru())
                .build(),
        }
    }

    #[cfg(test)]
    fn new_for_tests(max_capacity: u64, ttl: Duration) -> Self {
        Self::with_limits(max_capacity, ttl)
    }

    pub async fn get_selected_auth(&self, session_id: &str) -> Option<String> {
        self.selected_auth_by_session.get(session_id)
    }

    pub async fn set_selected_auth(&self, session_id: String, selected_auth_id: String) {
        self.selected_auth_by_session
            .insert(session_id, selected_auth_id);
        self.selected_auth_by_session.run_pending_tasks();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::ExecutionSessionStore;

    #[tokio::test]
    async fn execution_sessions_expire_after_ttl() {
        let store = ExecutionSessionStore::new_for_tests(16, Duration::from_millis(25));

        store
            .set_selected_auth("session-a".to_string(), "codex_0".to_string())
            .await;
        assert_eq!(store.get_selected_auth("session-a").await, Some("codex_0".to_string()));

        tokio::time::sleep(Duration::from_millis(40)).await;

        assert_eq!(store.get_selected_auth("session-a").await, None);
    }

    #[tokio::test]
    async fn execution_sessions_evict_when_capacity_is_exceeded() {
        let store = ExecutionSessionStore::new_for_tests(1, Duration::from_secs(60));

        store
            .set_selected_auth("session-a".to_string(), "codex_0".to_string())
            .await;
        store
            .set_selected_auth("session-b".to_string(), "codex_1".to_string())
            .await;

        assert_eq!(store.get_selected_auth("session-a").await, None);
        assert_eq!(store.get_selected_auth("session-b").await, Some("codex_1".to_string()));
    }
}
