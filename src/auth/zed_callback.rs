//! Zed native-app callback server.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{Context, Result};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use tokio::task::JoinHandle;

const ZED_SIGNIN_SUCCESS_REDIRECT: &str = "https://zed.dev/native_app_signin_succeeded";

#[derive(Clone, Debug)]
pub struct CallbackState {
    pub user_id: Arc<tokio::sync::Mutex<Option<String>>>,
    pub access_token: Arc<tokio::sync::Mutex<Option<String>>>,
    pub completed: Arc<AtomicBool>,
}

impl Default for CallbackState {
    fn default() -> Self {
        Self::new()
    }
}

impl CallbackState {
    pub fn new() -> Self {
        Self {
            user_id: Arc::new(tokio::sync::Mutex::new(None)),
            access_token: Arc::new(tokio::sync::Mutex::new(None)),
            completed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::SeqCst)
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    user_id: Option<String>,
    access_token: Option<String>,
}

pub async fn start_callback_server(port: u16) -> Result<(CallbackState, u16, JoinHandle<()>)> {
    let callback_state = CallbackState::new();
    let app = Router::new()
        .route("/", get(handle_callback))
        .with_state(callback_state.clone());

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind callback server on {addr}"))?;
    let actual_port = listener
        .local_addr()
        .context("failed to read callback listener address")?
        .port();

    let handle = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::warn!("zed callback server error: {error}");
        }
    });

    Ok((callback_state, actual_port, handle))
}

async fn handle_callback(
    State(state): State<CallbackState>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    if state.is_completed() {
        return (StatusCode::CONFLICT, "callback already completed").into_response();
    }

    let Some(user_id) = query
        .user_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return (StatusCode::BAD_REQUEST, "missing user_id query parameter").into_response();
    };

    let Some(access_token) = query
        .access_token
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            "missing access_token query parameter",
        )
            .into_response();
    };

    if state
        .completed
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return (StatusCode::CONFLICT, "callback already completed").into_response();
    }

    *state.user_id.lock().await = Some(user_id);
    *state.access_token.lock().await = Some(access_token);

    Redirect::temporary(ZED_SIGNIN_SUCCESS_REDIRECT).into_response()
}
