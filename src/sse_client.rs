//! SSE client for connecting to Mac MCP event stream.
//!
//! Provides automatic reconnection with exponential backoff
//! and sends parsed events through an mpsc channel.

// Module is prepared for integration in Task 5.5
#![allow(dead_code)]

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
    pub timestamp: String,
    pub data: serde_json::Value,
}

/// SSE client configuration.
#[derive(Debug, Clone)]
pub struct SseConfig {
    pub url: String,
    pub auth_token: String,
    pub subscriber_id: String,
}

/// Connect to SSE stream and send events to channel.
///
/// This function runs forever with automatic reconnection.
/// Events are sent through the provided mpsc channel.
///
/// # Reconnection Behavior
///
/// Uses exponential backoff starting at 1 second, doubling up to 30 seconds max.
/// Backoff resets to 1 second after a successful connection that ends cleanly.
///
/// # Arguments
///
/// * `config` - SSE connection configuration
/// * `tx` - Channel sender for parsed events
///
/// # Errors
///
/// This function runs indefinitely and only returns if the channel receiver is dropped.
pub async fn connect(config: SseConfig, tx: mpsc::Sender<Event>) -> Result<()> {
    let url = format!("{}/events?subscriber={}", config.url, config.subscriber_id);

    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(30);

    loop {
        info!("Connecting to SSE stream: {}", url);

        match connect_once(&url, &config.auth_token, &tx).await {
            Ok(()) => {
                // Clean disconnect, reset backoff
                backoff = Duration::from_secs(1);
            }
            Err(e) => {
                error!("SSE connection failed: {}", e);
            }
        }

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
        match event {
            Ok(SSE::Connected(details)) => {
                info!("SSE connected, status: {}", details.response().status());
            }
            Ok(SSE::Event(ev)) => {
                // Parse the event data as our Event struct
                match serde_json::from_str::<Event>(&ev.data) {
                    Ok(parsed_event) => {
                        if tx.send(parsed_event).await.is_err() {
                            // Receiver dropped, exit gracefully
                            info!("Event receiver dropped, closing SSE connection");
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse event data: {} - raw: {}", e, ev.data);
                    }
                }
            }
            Ok(SSE::Comment(_)) => {
                // Keepalive comment, ignore
            }
            Err(e) => {
                return Err(anyhow::anyhow!("SSE stream error: {e}"));
            }
        }
    }

    Ok(())
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
            auth_token: "test-token".to_string(),
            subscriber_id: "ludolph-pi".to_string(),
        };

        assert_eq!(config.url, "http://localhost:8080");
        assert_eq!(config.auth_token, "test-token");
        assert_eq!(config.subscriber_id, "ludolph-pi");
    }
}
