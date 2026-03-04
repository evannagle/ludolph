"""Tests for /events SSE endpoint."""

import os
import sys
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

# Add parent to path for package imports
sys.path.insert(0, str(Path(__file__).parent.parent.parent))


@pytest.fixture
def client():
    """Create test client with auth configured."""
    os.environ["VAULT_PATH"] = "/tmp/test-vault"
    os.environ["AUTH_TOKEN"] = "test-token"

    from mcp.security import init_security
    from mcp.server import app

    Path("/tmp/test-vault").mkdir(exist_ok=True)
    init_security(Path("/tmp/test-vault"), "test-token")

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
    data = response.get_json()
    assert data["error"] == "subscriber parameter required"


def test_events_returns_sse_stream(client):
    """Events endpoint returns SSE content type."""
    # Mock the event bus to avoid infinite loop in test
    mock_bus = MagicMock()
    mock_bus.receive.return_value = []

    with patch("mcp.server.get_event_bus", return_value=mock_bus):
        response = client.get(
            "/events?subscriber=test",
            headers={"Authorization": "Bearer test-token"}
        )

    assert response.status_code == 200
    assert response.content_type == "text/event-stream; charset=utf-8"


def test_events_subscribes_client_to_bus(client):
    """Events endpoint subscribes client to the event bus."""
    mock_bus = MagicMock()
    mock_bus.receive.return_value = []

    with patch("mcp.server.get_event_bus", return_value=mock_bus):
        client.get(
            "/events?subscriber=my-pi",
            headers={"Authorization": "Bearer test-token"}
        )

    mock_bus.subscribe.assert_called_once_with("my-pi")


def test_events_streams_published_events(client):
    """Events endpoint streams events from the bus."""
    from mcp.event_bus import Event

    mock_event = Event(
        id=1,
        type="channel_message",
        timestamp="2024-01-01T00:00:00",
        data={"from": "cc", "content": "hello"},
    )

    mock_bus = MagicMock()

    # Raise StopIteration after yielding one event to terminate generator
    def receive_once(*args, **kwargs):
        yield mock_event
        raise StopIteration

    mock_bus.receive.return_value = [mock_event]

    with patch("mcp.server.get_event_bus", return_value=mock_bus):
        # Use iter_encoded to get partial response without consuming all
        with client.get(
            "/events?subscriber=test",
            headers={"Authorization": "Bearer test-token"}
        ) as response:
            # Read just the first chunks
            chunks = []
            for i, chunk in enumerate(response.iter_encoded()):
                chunks.append(chunk.decode("utf-8"))
                if i >= 1:  # Get first couple chunks
                    break

    data = "".join(chunks)
    assert "channel_message" in data
    assert "hello" in data


def test_events_sends_keepalive(client):
    """Events endpoint sends keepalive comments."""
    mock_bus = MagicMock()
    mock_bus.receive.return_value = []

    with patch("mcp.server.get_event_bus", return_value=mock_bus):
        with client.get(
            "/events?subscriber=test",
            headers={"Authorization": "Bearer test-token"}
        ) as response:
            # Read just the first chunk which should include keepalive
            chunks = []
            for i, chunk in enumerate(response.iter_encoded()):
                chunks.append(chunk.decode("utf-8"))
                if ": keepalive" in chunk.decode("utf-8"):
                    break
                if i >= 5:  # Safety limit
                    break

    data = "".join(chunks)
    assert ": keepalive" in data
