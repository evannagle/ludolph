//! SSE client for connecting to Mac MCP event stream.
//!
//! Provides automatic reconnection with exponential backoff
//! and sends parsed events through an mpsc channel.

use anyhow::Result;
use eventsource_client::{Client as SseClient, ClientBuilder, SSE};
use futures::StreamExt;
use serde::Deserialize;
use std::pin::pin;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Event received from the MCP event stream.
#[derive(Debug, Clone, Deserialize)]
#[allow(clippy::struct_field_names)] // event_type matches JSON "type" field
pub struct Event {
    pub id: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    #[allow(dead_code)] // Protocol field, useful for logging/debugging
    pub timestamp: String,
    pub data: serde_json::Value,
}

/// SSE client configuration.
#[derive(Debug, Clone)]
pub struct SseConfig {
    pub url: String,
    pub fallback_url: Option<String>,
    pub auth_token: String,
    pub subscriber_id: String,
}

/// Tracks which URL the SSE client is currently connected through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Primary,
    Fallback,
    Disconnected,
}

/// Callback invoked when the SSE connection state changes.
pub type StateCallback = Box<dyn Fn(ConnectionState) + Send + Sync>;

/// Build the full SSE events URL from a base URL and subscriber ID.
fn build_url(base: &str, subscriber_id: &str) -> String {
    format!("{base}/events?subscriber={subscriber_id}")
}

/// Transition to a new connection state, invoking the callback if the state changed.
fn transition_state(
    current: &mut ConnectionState,
    next: ConnectionState,
    on_state_change: Option<&StateCallback>,
) {
    if *current != next {
        *current = next.clone();
        if let Some(cb) = on_state_change {
            cb(next);
        }
    }
}

/// Try connecting to a URL. Returns `true` if the connection succeeded (even if
/// it later disconnected cleanly), `false` if it failed to connect at all.
async fn try_connect(url: &str, label: &str, auth_token: &str, tx: &mpsc::Sender<Event>) -> bool {
    info!("Connecting to SSE stream ({}): {}", label, url);

    match connect_once(url, auth_token, tx).await {
        Ok(()) => true,
        Err(e) => {
            error!("SSE {} connection failed: {}", label, e);
            false
        }
    }
}

/// Connect to SSE stream and send events to channel.
///
/// This function runs forever with automatic reconnection.
/// Events are sent through the provided mpsc channel.
///
/// # Reconnection Behavior
///
/// Tries the primary URL first, then the fallback URL if configured.
/// Uses exponential backoff starting at 1 second, doubling up to 30 seconds max.
/// Backoff only applies when all available URLs fail. Backoff resets after a
/// successful connection that ends cleanly.
///
/// # Arguments
///
/// * `config` - SSE connection configuration
/// * `tx` - Channel sender for parsed events
/// * `on_state_change` - Optional callback invoked when connection state changes
///
/// # Errors
///
/// This function runs indefinitely and only returns if the channel receiver is dropped.
pub async fn connect(
    config: SseConfig,
    tx: mpsc::Sender<Event>,
    on_state_change: Option<StateCallback>,
) -> Result<()> {
    let primary_url = build_url(&config.url, &config.subscriber_id);
    let fallback_url = config
        .fallback_url
        .as_ref()
        .map(|base| build_url(base, &config.subscriber_id));

    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);
    let mut current_state = ConnectionState::Disconnected;

    loop {
        // Try primary URL
        if try_connect(&primary_url, "primary", &config.auth_token, &tx).await {
            backoff = Duration::from_secs(1);
            transition_state(
                &mut current_state,
                ConnectionState::Primary,
                on_state_change.as_ref(),
            );
            continue;
        }

        // Try fallback URL if configured
        if let Some(ref fb_url) = fallback_url {
            if try_connect(fb_url, "fallback", &config.auth_token, &tx).await {
                backoff = Duration::from_secs(1);
                transition_state(
                    &mut current_state,
                    ConnectionState::Fallback,
                    on_state_change.as_ref(),
                );
                continue;
            }
        }

        // Both URLs failed
        transition_state(
            &mut current_state,
            ConnectionState::Disconnected,
            on_state_change.as_ref(),
        );

        warn!("SSE disconnected, reconnecting in {:?}", backoff);
        tokio::time::sleep(backoff).await;

        // Exponential backoff with cap
        backoff = (backoff * 2).min(max_backoff);
    }
}

async fn connect_once(url: &str, auth_token: &str, tx: &mpsc::Sender<Event>) -> Result<()> {
    let client = ClientBuilder::for_url(url)?
        .header("Authorization", &format!("Bearer {auth_token}"))?
        .build();

    let mut stream = pin!(client.stream());

    while let Some(event) = stream.next().await {
        if !handle_sse_event(event, tx).await? {
            return Ok(());
        }
    }

    Ok(())
}

/// Handle a single SSE event. Returns `Ok(false)` if the connection should close.
async fn handle_sse_event(
    event: Result<SSE, eventsource_client::Error>,
    tx: &mpsc::Sender<Event>,
) -> Result<bool> {
    match event {
        Ok(SSE::Connected(details)) => {
            info!("SSE connected, status: {}", details.response().status());
        }
        Ok(SSE::Event(ev)) => {
            if !forward_event(&ev.data, tx).await {
                return Ok(false);
            }
        }
        Ok(SSE::Comment(_)) => {}
        Err(e) => {
            return Err(anyhow::anyhow!("SSE stream error: {e}"));
        }
    }
    Ok(true)
}

/// Parse and forward an event to the channel. Returns false if receiver is gone.
async fn forward_event(data: &str, tx: &mpsc::Sender<Event>) -> bool {
    match serde_json::from_str::<Event>(data) {
        Ok(parsed_event) => {
            if tx.send(parsed_event).await.is_err() {
                info!("Event receiver dropped, closing SSE connection");
                return false;
            }
        }
        Err(e) => {
            warn!("Failed to parse event data: {e} - raw: {data}");
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_deserializes_correctly() {
        let json = r#"{
            "id": 42,
            "type": "file_changed",
            "timestamp": "2024-01-15T10:30:00Z",
            "data": {"path": "/notes/todo.md"}
        }"#;

        let event: Event = serde_json::from_str(json).unwrap();

        assert_eq!(event.id, 42);
        assert_eq!(event.event_type, "file_changed");
        assert_eq!(event.timestamp, "2024-01-15T10:30:00Z");
        assert_eq!(event.data["path"], "/notes/todo.md");
    }

    #[test]
    fn event_deserializes_with_complex_data() {
        let json = r#"{
            "id": 1,
            "type": "notification",
            "timestamp": "2024-01-15T10:30:00Z",
            "data": {
                "title": "Test",
                "body": "Hello",
                "metadata": {"priority": "high", "count": 5}
            }
        }"#;

        let event: Event = serde_json::from_str(json).unwrap();

        assert_eq!(event.data["title"], "Test");
        assert_eq!(event.data["metadata"]["priority"], "high");
        assert_eq!(event.data["metadata"]["count"], 5);
    }

    #[test]
    fn sse_config_can_be_constructed() {
        let config = SseConfig {
            url: "http://localhost:8080".to_string(),
            fallback_url: None,
            auth_token: "test-token".to_string(),
            subscriber_id: "ludolph-pi".to_string(),
        };

        assert_eq!(config.url, "http://localhost:8080");
        assert_eq!(config.auth_token, "test-token");
        assert_eq!(config.subscriber_id, "ludolph-pi");
    }

    #[test]
    fn sse_config_with_fallback() {
        let config = SseConfig {
            url: "http://tailscale:8080".to_string(),
            fallback_url: Some("http://192.168.1.10:8080".to_string()),
            auth_token: "test-token".to_string(),
            subscriber_id: "ludolph-pi".to_string(),
        };

        assert_eq!(
            config.fallback_url.as_deref(),
            Some("http://192.168.1.10:8080")
        );
    }

    #[test]
    fn build_url_formats_correctly() {
        let url = build_url("http://localhost:8080", "pi_bot");
        assert_eq!(url, "http://localhost:8080/events?subscriber=pi_bot");
    }

    #[test]
    fn build_url_handles_trailing_content() {
        let url = build_url("http://example.com:9000", "my-subscriber");
        assert_eq!(
            url,
            "http://example.com:9000/events?subscriber=my-subscriber"
        );
    }

    #[test]
    fn connection_state_equality() {
        assert_eq!(ConnectionState::Primary, ConnectionState::Primary);
        assert_eq!(ConnectionState::Fallback, ConnectionState::Fallback);
        assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
        assert_ne!(ConnectionState::Primary, ConnectionState::Fallback);
    }
}
