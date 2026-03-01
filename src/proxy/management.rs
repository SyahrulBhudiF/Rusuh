use axum::{Json, Router, extract::State, routing::get};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::proxy::ProxyState;

pub fn router() -> Router<Arc<ProxyState>> {
    Router::new()
        .route("/status", get(status))
        .route("/config", get(get_config))
}

async fn status(State(state): State<Arc<ProxyState>>) -> Json<Value> {
    let cfg = state.config.read().await;
    Json(json!({
        "status": "running",
        "port": cfg.port,
        "providers": state.providers.len(),
    }))
}

async fn get_config(State(state): State<Arc<ProxyState>>) -> Json<Value> {
    let cfg = state.config.read().await;
    Json(json!({
        "port": cfg.port,
        "host": cfg.host,
        "debug": cfg.debug,
    }))
}
