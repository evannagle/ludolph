# Event Bus & Channel Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build two-way communication between Claude Code and Lu via a generic event bus on Mac MCP.

**Architecture:** Mac MCP serves as hub with SSE streaming. Event bus handles pub/sub, channel stores messages and logs to vault. Pi connects via SSE, dispatches events to handlers.

**Tech Stack:** Python (Flask, threading), Rust (reqwest, tokio), SSE protocol

---

## Part 1: Mac MCP - Event Bus

### Task 1.1: Create Event Bus Module

**Files:**
- Create: `src/mcp/event_bus.py`
- Test: `src/mcp/tests/test_event_bus.py`

**Step 1: Write the failing test**

```python
# src/mcp/tests/test_event_bus.py
"""Tests for event bus."""

import pytest
from mcp.event_bus import EventBus, Event


def test_publish_and_receive():
    """Published events are received by subscribers."""
    bus = EventBus()
    bus.subscribe("test_sub")

    event = bus.publish("channel_message", {"from": "cc", "content": "hi"})

    assert event.id == 1
    assert event.type == "channel_message"

    events = bus.receive("test_sub")
    assert len(events) == 1
    assert events[0].data["content"] == "hi"


def test_subscriber_does_not_receive_own_events():
    """Subscribers don't receive events they published."""
    bus = EventBus()
    bus.subscribe("sender")

    bus.publish("channel_message", {"from": "sender", "content": "hi"}, source="sender")

    events = bus.receive("sender")
    assert len(events) == 0


def test_events_marked_as_read():
    """Events are marked read after receive."""
    bus = EventBus()
    bus.subscribe("test_sub")

    bus.publish("test", {"msg": "hello"})

    events1 = bus.receive("test_sub")
    assert len(events1) == 1

    events2 = bus.receive("test_sub")
    assert len(events2) == 0
```

**Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_event_bus.py -v`
Expected: FAIL with "No module named 'mcp.event_bus'"

**Step 3: Write minimal implementation**

```python
# src/mcp/event_bus.py
"""Generic event bus with pub/sub support."""

import threading
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any


@dataclass
class Event:
    """An event in the bus."""
    id: int
    type: str
    timestamp: str
    data: dict[str, Any]
    source: str | None = None
    read_by: list[str] = field(default_factory=list)


class EventBus:
    """Thread-safe pub/sub event bus."""

    def __init__(self, max_events: int = 1000):
        self._events: deque[Event] = deque(maxlen=max_events)
        self._next_id = 1
        self._lock = threading.Lock()
        self._subscribers: set[str] = set()

    def subscribe(self, subscriber_id: str) -> None:
        """Register a subscriber."""
        with self._lock:
            self._subscribers.add(subscriber_id)

    def unsubscribe(self, subscriber_id: str) -> None:
        """Remove a subscriber."""
        with self._lock:
            self._subscribers.discard(subscriber_id)

    def publish(
        self,
        event_type: str,
        data: dict[str, Any],
        source: str | None = None
    ) -> Event:
        """Publish an event to all subscribers."""
        with self._lock:
            event = Event(
                id=self._next_id,
                type=event_type,
                timestamp=datetime.now().isoformat(),
                data=data,
                source=source,
                read_by=[source] if source else [],
            )
            self._next_id += 1
            self._events.append(event)
            return event

    def receive(
        self,
        subscriber_id: str,
        since_id: int = 0,
        mark_read: bool = True
    ) -> list[Event]:
        """Get unread events for a subscriber."""
        with self._lock:
            events = []
            for event in self._events:
                if event.id <= since_id:
                    continue
                if subscriber_id in event.read_by:
                    continue
                events.append(event)
                if mark_read:
                    event.read_by.append(subscriber_id)
            return events

    def get_recent(self, limit: int = 20) -> list[Event]:
        """Get recent events regardless of read status."""
        with self._lock:
            return list(self._events)[-limit:]


# Global instance
_bus: EventBus | None = None


def get_event_bus() -> EventBus:
    """Get or create the global event bus."""
    global _bus
    if _bus is None:
        _bus = EventBus()
    return _bus
```

**Step 4: Run test to verify it passes**

Run: `cd src/mcp && python -m pytest tests/test_event_bus.py -v`
Expected: PASS (3 tests)

**Step 5: Commit**

```bash
git add src/mcp/event_bus.py src/mcp/tests/test_event_bus.py
git commit -m "feat(mcp): add event bus with pub/sub"
```

---

## Part 2: Mac MCP - Channel

### Task 2.1: Create Channel Module

**Files:**
- Create: `src/mcp/channel.py`
- Test: `src/mcp/tests/test_channel.py`

**Step 1: Write the failing test**

```python
# src/mcp/tests/test_channel.py
"""Tests for channel messaging."""

import pytest
from unittest.mock import patch, MagicMock
from pathlib import Path

from mcp.channel import Channel, ChannelMessage


def test_send_creates_message():
    """Sending creates a message and publishes event."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=None)

    msg = channel.send("claude_code", "Hello Lu")

    assert msg.id == 1
    assert msg.sender == "claude_code"
    assert msg.content == "Hello Lu"
    mock_bus.publish.assert_called_once()


def test_history_returns_recent_messages():
    """History returns messages in order."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=None)

    channel.send("cc", "msg1")
    channel.send("lu", "msg2")
    channel.send("cc", "msg3")

    history = channel.history(limit=2)

    assert len(history) == 2
    assert history[0].content == "msg2"
    assert history[1].content == "msg3"


def test_send_logs_to_vault(tmp_path):
    """Messages are logged to vault."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=tmp_path)

    channel.send("claude_code", "Test message")

    log_files = list((tmp_path / ".lu" / "channel").glob("*.md"))
    assert len(log_files) == 1

    content = log_files[0].read_text()
    assert "claude_code" in content
    assert "Test message" in content
```

**Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_channel.py -v`
Expected: FAIL with "No module named 'mcp.channel'"

**Step 3: Write minimal implementation**

```python
# src/mcp/channel.py
"""Channel messaging between Claude Code and Lu."""

import logging
import threading
from collections import deque
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .event_bus import EventBus

logger = logging.getLogger(__name__)

CHANNEL_LOG_DIR = ".lu/channel"
MAX_MESSAGES = 500


@dataclass
class ChannelMessage:
    """A message in the channel."""
    id: int
    sender: str
    content: str
    timestamp: str
    reply_to: int | None = None


class Channel:
    """Bidirectional message channel with vault logging."""

    def __init__(
        self,
        event_bus: "EventBus",
        vault_path: Path | None = None
    ):
        self._bus = event_bus
        self._vault_path = vault_path
        self._messages: deque[ChannelMessage] = deque(maxlen=MAX_MESSAGES)
        self._next_id = 1
        self._lock = threading.Lock()

    def send(
        self,
        sender: str,
        content: str,
        reply_to: int | None = None
    ) -> ChannelMessage:
        """Send a message to the channel."""
        with self._lock:
            msg = ChannelMessage(
                id=self._next_id,
                sender=sender,
                content=content,
                timestamp=datetime.now().isoformat(),
                reply_to=reply_to,
            )
            self._next_id += 1
            self._messages.append(msg)

        # Publish event
        self._bus.publish(
            "channel_message",
            {
                "id": msg.id,
                "from": msg.sender,
                "content": msg.content,
                "reply_to": msg.reply_to,
            },
            source=sender,
        )

        # Log to vault
        self._log_to_vault(msg)

        return msg

    def history(self, limit: int = 20) -> list[ChannelMessage]:
        """Get recent message history."""
        with self._lock:
            return list(self._messages)[-limit:]

    def _log_to_vault(self, msg: ChannelMessage) -> None:
        """Log message to vault for searchability."""
        if not self._vault_path:
            return

        try:
            log_dir = self._vault_path / CHANNEL_LOG_DIR
            log_dir.mkdir(parents=True, exist_ok=True)

            today = datetime.now().strftime("%Y-%m-%d")
            log_file = log_dir / f"{today}.md"

            timestamp = msg.timestamp[11:19]

            # Determine direction arrow
            if msg.sender == "lu":
                direction = "lu → claude_code"
            else:
                direction = f"{msg.sender} → lu"

            entry = f"\n### {timestamp} [{direction}]\n\n{msg.content}\n"

            with open(log_file, "a") as f:
                if log_file.stat().st_size == 0:
                    f.write(f"# Channel Log - {today}\n")
                f.write(entry)

        except Exception as e:
            logger.warning(f"Failed to log channel message: {e}")


# Global instance
_channel: Channel | None = None


def get_channel(event_bus: "EventBus", vault_path: Path | None = None) -> Channel:
    """Get or create the global channel."""
    global _channel
    if _channel is None:
        _channel = Channel(event_bus, vault_path)
    return _channel
```

**Step 4: Run test to verify it passes**

Run: `cd src/mcp && python -m pytest tests/test_channel.py -v`
Expected: PASS (3 tests)

**Step 5: Commit**

```bash
git add src/mcp/channel.py src/mcp/tests/test_channel.py
git commit -m "feat(mcp): add channel messaging with vault logging"
```

---

## Part 3: Mac MCP - Server Endpoints

### Task 3.1: Add SSE Events Endpoint

**Files:**
- Modify: `src/mcp/server.py`
- Test: `src/mcp/tests/test_server_events.py`

**Step 1: Write the failing test**

```python
# src/mcp/tests/test_server_events.py
"""Tests for /events SSE endpoint."""

import pytest
from unittest.mock import patch, MagicMock
import json


@pytest.fixture
def client():
    """Create test client."""
    import os
    os.environ.setdefault("VAULT_PATH", "/tmp/test_vault")
    os.environ.setdefault("AUTH_TOKEN", "test-token")

    from mcp.server import app
    app.config["TESTING"] = True
    with app.test_client() as client:
        yield client


def test_events_requires_auth(client):
    """Events endpoint requires authentication."""
    response = client.get("/events?subscriber=test")
    assert response.status_code == 401


def test_events_requires_subscriber(client):
    """Events endpoint requires subscriber parameter."""
    response = client.get(
        "/events",
        headers={"Authorization": "Bearer test-token"}
    )
    assert response.status_code == 400


def test_events_returns_sse_stream(client):
    """Events endpoint returns SSE content type."""
    response = client.get(
        "/events?subscriber=test",
        headers={"Authorization": "Bearer test-token"}
    )
    assert response.status_code == 200
    assert response.content_type == "text/event-stream"
```

**Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_server_events.py -v`
Expected: FAIL with 404 for /events

**Step 3: Add /events endpoint to server.py**

Add these imports at top of `src/mcp/server.py`:

```python
from .event_bus import get_event_bus
from .channel import get_channel
```

Add this endpoint after existing routes:

```python
@app.route("/events", methods=["GET"])
@require_auth
def events():
    """SSE stream of events for a subscriber."""
    subscriber = request.args.get("subscriber")
    if not subscriber:
        return jsonify({"error": "subscriber parameter required"}), 400

    bus = get_event_bus()
    bus.subscribe(subscriber)

    def generate():
        import time
        last_id = 0
        while True:
            events = bus.receive(subscriber, since_id=last_id)
            for event in events:
                last_id = event.id
                data = {
                    "id": event.id,
                    "type": event.type,
                    "timestamp": event.timestamp,
                    "data": event.data,
                }
                yield f"data: {json.dumps(data)}\n\n"

            # Keepalive every 30 seconds
            yield ": keepalive\n\n"
            time.sleep(1)

    return Response(generate(), mimetype="text/event-stream")
```

**Step 4: Run test to verify it passes**

Run: `cd src/mcp && python -m pytest tests/test_server_events.py -v`
Expected: PASS (3 tests)

**Step 5: Commit**

```bash
git add src/mcp/server.py src/mcp/tests/test_server_events.py
git commit -m "feat(mcp): add /events SSE endpoint"
```

---

### Task 3.2: Add Channel Endpoints

**Files:**
- Modify: `src/mcp/server.py`
- Test: `src/mcp/tests/test_server_channel.py`

**Step 1: Write the failing test**

```python
# src/mcp/tests/test_server_channel.py
"""Tests for /channel/* endpoints."""

import pytest
from unittest.mock import patch, MagicMock
import json


@pytest.fixture
def client():
    """Create test client."""
    import os
    os.environ.setdefault("VAULT_PATH", "/tmp/test_vault")
    os.environ.setdefault("AUTH_TOKEN", "test-token")

    from mcp.server import app
    app.config["TESTING"] = True
    with app.test_client() as client:
        yield client


def test_channel_send_requires_auth(client):
    """Channel send requires authentication."""
    response = client.post("/channel/send", json={"from": "test", "content": "hi"})
    assert response.status_code == 401


def test_channel_send_creates_message(client):
    """Channel send creates and returns message."""
    response = client.post(
        "/channel/send",
        json={"from": "claude_code", "content": "Hello Lu"},
        headers={"Authorization": "Bearer test-token"}
    )
    assert response.status_code == 200
    data = response.get_json()
    assert data["status"] == "sent"
    assert "id" in data


def test_channel_history_returns_messages(client):
    """Channel history returns recent messages."""
    # Send a message first
    client.post(
        "/channel/send",
        json={"from": "test", "content": "test msg"},
        headers={"Authorization": "Bearer test-token"}
    )

    response = client.get(
        "/channel/history?limit=10",
        headers={"Authorization": "Bearer test-token"}
    )
    assert response.status_code == 200
    data = response.get_json()
    assert "messages" in data
```

**Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_server_channel.py -v`
Expected: FAIL with 404 for /channel/send

**Step 3: Add channel endpoints to server.py**

```python
@app.route("/channel/send", methods=["POST"])
@require_auth
def channel_send():
    """Send a message to the channel."""
    data = request.json or {}
    sender = data.get("from")
    content = data.get("content")
    reply_to = data.get("reply_to")

    if not sender or not content:
        return jsonify({"error": "from and content required"}), 400

    bus = get_event_bus()
    channel = get_channel(bus, get_vault_path())

    msg = channel.send(sender, content, reply_to)

    return jsonify({
        "status": "sent",
        "id": msg.id,
        "timestamp": msg.timestamp,
    })


@app.route("/channel/history", methods=["GET"])
@require_auth
def channel_history():
    """Get recent channel message history."""
    limit = request.args.get("limit", 20, type=int)

    bus = get_event_bus()
    channel = get_channel(bus, get_vault_path())

    messages = channel.history(limit)

    return jsonify({
        "messages": [
            {
                "id": m.id,
                "from": m.sender,
                "content": m.content,
                "timestamp": m.timestamp,
                "reply_to": m.reply_to,
            }
            for m in messages
        ]
    })
```

**Step 4: Run test to verify it passes**

Run: `cd src/mcp && python -m pytest tests/test_server_channel.py -v`
Expected: PASS (3 tests)

**Step 5: Commit**

```bash
git add src/mcp/server.py src/mcp/tests/test_server_channel.py
git commit -m "feat(mcp): add /channel/send and /channel/history endpoints"
```

---

## Part 4: Mac MCP - Channel Tools

### Task 4.1: Create Channel MCP Tools

**Files:**
- Create: `src/mcp/tools/channel.py`
- Modify: `src/mcp/tools/__init__.py`

**Step 1: Write the tool module**

```python
# src/mcp/tools/channel.py
"""MCP tools for channel messaging.

Allows Claude Code to send messages to Lu and view conversation history.
"""

import logging
from typing import Any

from ..event_bus import get_event_bus
from ..channel import get_channel
from ..security import get_vault_path

logger = logging.getLogger(__name__)

TOOLS = [
    {
        "name": "channel_send",
        "description": "Send a message to Lu via the channel. Lu will process and respond automatically.",
        "input_schema": {
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The message to send to Lu",
                },
                "reply_to": {
                    "type": "integer",
                    "description": "Optional message ID this is replying to",
                },
            },
            "required": ["content"],
        },
    },
    {
        "name": "channel_history",
        "description": "Get recent channel conversation history between Claude Code and Lu.",
        "input_schema": {
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "description": "Maximum messages to return (default 20)",
                    "default": 20,
                },
            },
        },
    },
]


def _handle_channel_send(args: dict[str, Any]) -> dict:
    """Send a message to the channel."""
    content = args.get("content", "").strip()
    reply_to = args.get("reply_to")

    if not content:
        return {"content": "", "error": "Message content is required"}

    try:
        bus = get_event_bus()
        channel = get_channel(bus, get_vault_path())

        msg = channel.send("claude_code", content, reply_to)

        return {
            "content": f"Message sent (ID: {msg.id}). Lu will respond shortly.\n\nCheck channel_history or read .lu/channel/ for responses.",
            "error": None,
        }
    except Exception as e:
        logger.error(f"Channel send failed: {e}")
        return {"content": "", "error": str(e)}


def _handle_channel_history(args: dict[str, Any]) -> dict:
    """Get channel conversation history."""
    limit = args.get("limit", 20)

    try:
        bus = get_event_bus()
        channel = get_channel(bus, get_vault_path())

        messages = channel.history(limit)

        if not messages:
            return {"content": "No channel messages yet.", "error": None}

        lines = ["Channel History:\n"]
        for msg in messages:
            ts = msg.timestamp[11:19]
            direction = "→ lu" if msg.sender != "lu" else "← lu"
            lines.append(f"[{ts}] {msg.sender} {direction}: {msg.content[:100]}")
            if len(msg.content) > 100:
                lines[-1] += "..."

        return {"content": "\n".join(lines), "error": None}
    except Exception as e:
        logger.error(f"Channel history failed: {e}")
        return {"content": "", "error": str(e)}


HANDLERS = {
    "channel_send": _handle_channel_send,
    "channel_history": _handle_channel_history,
}
```

**Step 2: Add to tools/__init__.py**

Add import:
```python
from . import channel
```

Add to `_CORE_TOOLS`:
```python
+ channel.TOOLS
```

Add to `_CORE_HANDLERS`:
```python
**channel.HANDLERS,
```

**Step 3: Verify tools load**

Run: `python3 -c "from mcp.tools import channel; print(f'Loaded {len(channel.TOOLS)} tools')"`
Expected: `Loaded 2 tools`

**Step 4: Commit**

```bash
git add src/mcp/tools/channel.py src/mcp/tools/__init__.py
git commit -m "feat(mcp): add channel_send and channel_history tools"
```

---

## Part 5: Pi Bot - SSE Client

### Task 5.1: Add SSE Client Dependency

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add eventsource-client crate**

```toml
# Add to [dependencies]
eventsource-client = "0.13"
```

**Step 2: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add eventsource-client for SSE support"
```

---

### Task 5.2: Create SSE Client Module

**Files:**
- Create: `src/sse_client.rs`
- Modify: `src/main.rs` (add module)

**Step 1: Create SSE client**

```rust
// src/sse_client.rs
//! SSE client for connecting to Mac MCP event stream.

use anyhow::{Context, Result};
use eventsource_client::{Client, SSE};
use futures::StreamExt;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Event received from the MCP event stream.
#[derive(Debug, Clone, Deserialize)]
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
pub async fn connect(
    config: SseConfig,
    tx: mpsc::Sender<Event>,
) -> Result<()> {
    let url = format!(
        "{}/events?subscriber={}",
        config.url, config.subscriber_id
    );

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

        // Exponential backoff
        backoff = (backoff * 2).min(max_backoff);
    }
}

async fn connect_once(
    url: &str,
    auth_token: &str,
    tx: &mpsc::Sender<Event>,
) -> Result<()> {
    let client = Client::for_url(url)?
        .header("Authorization", &format!("Bearer {}", auth_token))?
        .build();

    let mut stream = client.stream();

    while let Some(event) = stream.next().await {
        match event {
            Ok(SSE::Event(ev)) => {
                if ev.data.starts_with(':') {
                    // Keepalive comment, ignore
                    continue;
                }

                match serde_json::from_str::<Event>(&ev.data) {
                    Ok(event) => {
                        if tx.send(event).await.is_err() {
                            // Receiver dropped
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse event: {}", e);
                    }
                }
            }
            Ok(SSE::Comment(_)) => {
                // Keepalive, ignore
            }
            Err(e) => {
                return Err(anyhow::anyhow!("SSE error: {}", e));
            }
        }
    }

    Ok(())
}
```

**Step 2: Add module to main.rs**

Add: `mod sse_client;`

**Step 3: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 4: Commit**

```bash
git add src/sse_client.rs src/main.rs
git commit -m "feat: add SSE client for MCP event stream"
```

---

### Task 5.3: Create Event Handler Module

**Files:**
- Create: `src/event_handler.rs`
- Modify: `src/main.rs` (add module)

**Step 1: Create event handler**

```rust
// src/event_handler.rs
//! Event handler for processing MCP events.

use crate::llm::LlmClient;
use crate::mcp_client::McpClient;
use crate::sse_client::Event;
use anyhow::Result;
use serde::Deserialize;
use tracing::{info, warn};

/// Channel message data from event.
#[derive(Debug, Deserialize)]
struct ChannelMessageData {
    id: u64,
    from: String,
    content: String,
    reply_to: Option<u64>,
}

/// Handle an event from the MCP stream.
pub async fn handle_event(
    event: Event,
    llm: &LlmClient,
    mcp: &McpClient,
) -> Result<()> {
    match event.event_type.as_str() {
        "channel_message" => {
            handle_channel_message(event.data, llm, mcp).await?;
        }
        "system_status" => {
            info!("System status: {:?}", event.data);
        }
        _ => {
            // Unknown event type, ignore
        }
    }
    Ok(())
}

async fn handle_channel_message(
    data: serde_json::Value,
    llm: &LlmClient,
    mcp: &McpClient,
) -> Result<()> {
    let msg: ChannelMessageData = serde_json::from_value(data)?;

    // Don't respond to our own messages
    if msg.from == "lu" {
        return Ok(());
    }

    info!("Channel message from {}: {}", msg.from, msg.content);

    // Process through LLM (same as Telegram messages)
    let response = llm.chat(&msg.content, None).await?;

    // Send response back to channel
    mcp.channel_send("lu", &response.response, Some(msg.id)).await?;

    Ok(())
}
```

**Step 2: Add module to main.rs**

Add: `mod event_handler;`

**Step 3: Verify build**

Run: `cargo build`
Expected: Compiles successfully (may have unused warnings, that's OK)

**Step 4: Commit**

```bash
git add src/event_handler.rs src/main.rs
git commit -m "feat: add event handler for channel messages"
```

---

### Task 5.4: Add Channel Send to MCP Client

**Files:**
- Modify: `src/mcp_client.rs`

**Step 1: Add channel_send method**

```rust
/// Send a message to the channel.
pub async fn channel_send(
    &self,
    from: &str,
    content: &str,
    reply_to: Option<u64>,
) -> Result<()> {
    let url = format!("{}/channel/send", self.base_url);

    let mut body = serde_json::json!({
        "from": from,
        "content": content,
    });

    if let Some(id) = reply_to {
        body["reply_to"] = serde_json::json!(id);
    }

    let response = self
        .client
        .post(&url)
        .header("Authorization", format!("Bearer {}", self.token))
        .json(&body)
        .send()
        .await
        .context("Failed to send channel message")?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("Channel send failed: {} - {}", status, text);
    }

    Ok(())
}
```

**Step 2: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 3: Commit**

```bash
git add src/mcp_client.rs
git commit -m "feat(mcp_client): add channel_send method"
```

---

### Task 5.5: Integrate SSE into Bot

**Files:**
- Modify: `src/bot.rs`

**Step 1: Spawn SSE task in bot startup**

Add to bot initialization (after MCP client setup):

```rust
// Spawn SSE event listener if MCP is configured
if let Some(mcp_config) = &mcp_config {
    let sse_config = crate::sse_client::SseConfig {
        url: mcp_config.url.clone(),
        auth_token: mcp_config.auth_token.clone(),
        subscriber_id: "pi_bot".to_string(),
    };

    let llm_clone = llm.clone();
    let mcp_clone = McpClient::from_config(mcp_config);

    tokio::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);

        // Spawn SSE connection
        let sse_config_clone = sse_config.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::sse_client::connect(sse_config_clone, tx).await {
                tracing::error!("SSE client error: {}", e);
            }
        });

        // Process events
        while let Some(event) = rx.recv().await {
            if let Err(e) = crate::event_handler::handle_event(
                event,
                &llm_clone,
                &mcp_clone,
            ).await {
                tracing::error!("Event handler error: {}", e);
            }
        }
    });
}
```

**Step 2: Verify build**

Run: `cargo build`
Expected: Compiles successfully

**Step 3: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 4: Commit**

```bash
git add src/bot.rs
git commit -m "feat(bot): spawn SSE listener for channel events"
```

---

## Part 6: Deploy and Test

### Task 6.1: Deploy Mac MCP Updates

**Step 1: Copy files to deployed location**

```bash
cp src/mcp/event_bus.py ~/.ludolph/mcp/
cp src/mcp/channel.py ~/.ludolph/mcp/
cp src/mcp/tools/channel.py ~/.ludolph/mcp/tools/
cp src/mcp/tools/__init__.py ~/.ludolph/mcp/tools/
cp src/mcp/server.py ~/.ludolph/mcp/
```

**Step 2: Restart MCP server**

```bash
launchctl kickstart -k gui/$(id -u)/dev.ludolph.mcp
```

**Step 3: Verify tools loaded**

```bash
curl -s http://localhost:8201/tools \
  -H "Authorization: Bearer $(cat ~/.ludolph/mcp_token)" \
  | python3 -c "import sys,json; tools=json.load(sys.stdin)['tools']; print([t['name'] for t in tools if 'channel' in t['name']])"
```

Expected: `['channel_send', 'channel_history']`

**Step 4: Test channel send**

```bash
curl -X POST http://localhost:8201/channel/send \
  -H "Authorization: Bearer $(cat ~/.ludolph/mcp_token)" \
  -H "Content-Type: application/json" \
  -d '{"from":"test","content":"Hello from test"}'
```

Expected: `{"id": 1, "status": "sent", "timestamp": "..."}`

**Step 5: Check vault log**

```bash
cat ~/Vaults/Noggin/noggin/.lu/channel/$(date +%Y-%m-%d).md
```

Expected: Channel log with test message

**Step 6: Commit deploy verification**

```bash
git add -A
git commit -m "test: verify Mac MCP channel deployment"
```

---

### Task 6.2: Deploy Pi Bot Updates

**Step 1: Build release on Pi**

```bash
ssh pi "cd ~/ludolph && git pull && cargo build --release"
```

**Step 2: Deploy and restart**

```bash
ssh pi "systemctl --user stop ludolph && cp ~/ludolph/target/release/lu ~/.ludolph/bin/ && systemctl --user start ludolph"
```

**Step 3: Check logs for SSE connection**

```bash
ssh pi "journalctl --user -u ludolph -n 20 | grep -i sse"
```

Expected: "Connecting to SSE stream" message

---

### Task 6.3: End-to-End Test

**Step 1: Send message from Claude Code**

Use channel_send tool or:
```bash
curl -X POST http://localhost:8201/channel/send \
  -H "Authorization: Bearer $(cat ~/.ludolph/mcp_token)" \
  -H "Content-Type: application/json" \
  -d '{"from":"claude_code","content":"Hey Lu, can you search for notes about project planning?"}'
```

**Step 2: Check Pi logs for processing**

```bash
ssh pi "journalctl --user -u ludolph -n 20"
```

Expected: "Channel message from claude_code" log

**Step 3: Check vault log for response**

```bash
cat ~/Vaults/Noggin/noggin/.lu/channel/$(date +%Y-%m-%d).md
```

Expected: Both CC message and Lu response logged

**Step 4: Final commit**

```bash
git add -A
git commit -m "feat: complete event bus and channel implementation"
```

---

## Verification Checklist

- [ ] Event bus tests pass
- [ ] Channel tests pass
- [ ] Server endpoint tests pass
- [ ] MCP tools load correctly
- [ ] Pi builds without errors
- [ ] Pi connects to SSE stream
- [ ] Channel messages flow CC → Lu
- [ ] Lu responses flow back to vault log
- [ ] Vault log searchable
