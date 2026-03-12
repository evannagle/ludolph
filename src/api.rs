//! HTTP API for channel messaging.
//!
//! Exposes endpoints for Claude Code to send messages and read history.
//! When messages arrive from external senders (not "lu"), a notification
//! is sent to trigger LLM processing.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelMessage};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub channel: Channel,
    pub auth_token: String,
}

/// Request body for sending a message.
#[derive(Debug, Deserialize)]
pub struct SendRequest {
    pub from: String,
    pub content: String,
    #[serde(default)]
    pub reply_to: Option<u64>,
    #[serde(default)]
    pub context: Option<serde_json::Value>,
}

/// Response for send endpoint.
#[derive(Debug, Serialize)]
pub struct SendResponse {
    pub status: String,
    pub id: u64,
    pub timestamp: String,
}

/// Query params for history endpoint.
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
}

const fn default_limit() -> usize {
    20
}

/// Response for history endpoint.
#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub messages: Vec<ChannelMessage>,
}

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Check authorization header.
fn check_auth(headers: &HeaderMap, expected_token: &str) -> Result<(), StatusCode> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let token = auth
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if token != expected_token {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(())
}

/// Health check endpoint.
async fn health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Send a message to the channel.
async fn channel_send(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<SendRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    check_auth(&headers, &state.auth_token)?;

    let msg = state
        .channel
        .send(&req.from, &req.content, req.reply_to, req.context);

    Ok(Json(SendResponse {
        status: "sent".to_string(),
        id: msg.id,
        timestamp: msg.timestamp.to_rfc3339(),
    }))
}

/// Get channel message history.
async fn channel_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    check_auth(&headers, &state.auth_token)?;

    let messages = state.channel.history(query.limit);

    Ok(Json(HistoryResponse { messages }))
}

/// Create the API router.
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/channel/send", post(channel_send))
        .route("/channel/history", get(channel_history))
        .with_state(state)
}

/// Run the API server.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve.
pub async fn run_server(state: Arc<AppState>, port: u16) -> anyhow::Result<()> {
    let router = create_router(state);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));

    tracing::info!("Channel API listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
