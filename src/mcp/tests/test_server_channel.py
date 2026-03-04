"""Tests for /channel/* endpoints."""

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
    from mcp.channel import reset_channel

    # Reset channel state between tests
    reset_channel()

    Path("/tmp/test-vault").mkdir(exist_ok=True)
    init_security(Path("/tmp/test-vault"), "test-token")

    app.config["TESTING"] = True
    with app.test_client() as client:
        yield client


def test_channel_send_requires_auth(client):
    """Channel send requires authentication."""
    response = client.post("/channel/send", json={"from": "test", "content": "hi"})
    assert response.status_code == 401


def test_channel_send_creates_message(client):
    """Channel send creates and returns message."""
    mock_bus = MagicMock()
    mock_channel = MagicMock()
    mock_msg = MagicMock()
    mock_msg.id = 1
    mock_msg.timestamp = "2024-01-01T12:00:00"
    mock_channel.send.return_value = mock_msg

    with patch("mcp.server.get_event_bus", return_value=mock_bus), \
         patch("mcp.server.get_channel", return_value=mock_channel):
        response = client.post(
            "/channel/send",
            json={"from": "claude_code", "content": "Hello Lu"},
            headers={"Authorization": "Bearer test-token"}
        )

    assert response.status_code == 200
    data = response.get_json()
    assert data["status"] == "sent"
    assert data["id"] == 1
    assert "timestamp" in data


def test_channel_send_requires_from_field(client):
    """Channel send requires 'from' field."""
    response = client.post(
        "/channel/send",
        json={"content": "Hello Lu"},
        headers={"Authorization": "Bearer test-token"}
    )
    assert response.status_code == 400
    data = response.get_json()
    assert "error" in data


def test_channel_send_requires_content_field(client):
    """Channel send requires 'content' field."""
    response = client.post(
        "/channel/send",
        json={"from": "claude_code"},
        headers={"Authorization": "Bearer test-token"}
    )
    assert response.status_code == 400
    data = response.get_json()
    assert "error" in data


def test_channel_send_passes_reply_to(client):
    """Channel send passes reply_to to channel."""
    mock_bus = MagicMock()
    mock_channel = MagicMock()
    mock_msg = MagicMock()
    mock_msg.id = 2
    mock_msg.timestamp = "2024-01-01T12:00:01"
    mock_channel.send.return_value = mock_msg

    with patch("mcp.server.get_event_bus", return_value=mock_bus), \
         patch("mcp.server.get_channel", return_value=mock_channel):
        response = client.post(
            "/channel/send",
            json={"from": "lu", "content": "Reply", "reply_to": 1},
            headers={"Authorization": "Bearer test-token"}
        )

    assert response.status_code == 200
    mock_channel.send.assert_called_once_with("lu", "Reply", 1)


def test_channel_history_requires_auth(client):
    """Channel history requires authentication."""
    response = client.get("/channel/history")
    assert response.status_code == 401


def test_channel_history_returns_messages(client):
    """Channel history returns recent messages."""
    mock_bus = MagicMock()
    mock_channel = MagicMock()
    mock_msg = MagicMock()
    mock_msg.id = 1
    mock_msg.sender = "test"
    mock_msg.content = "test msg"
    mock_msg.timestamp = "2024-01-01T12:00:00"
    mock_msg.reply_to = None
    mock_channel.history.return_value = [mock_msg]

    with patch("mcp.server.get_event_bus", return_value=mock_bus), \
         patch("mcp.server.get_channel", return_value=mock_channel):
        response = client.get(
            "/channel/history?limit=10",
            headers={"Authorization": "Bearer test-token"}
        )

    assert response.status_code == 200
    data = response.get_json()
    assert "messages" in data
    assert len(data["messages"]) == 1
    assert data["messages"][0]["content"] == "test msg"
    mock_channel.history.assert_called_once_with(10)


def test_channel_history_default_limit(client):
    """Channel history uses default limit of 20."""
    mock_bus = MagicMock()
    mock_channel = MagicMock()
    mock_channel.history.return_value = []

    with patch("mcp.server.get_event_bus", return_value=mock_bus), \
         patch("mcp.server.get_channel", return_value=mock_channel):
        response = client.get(
            "/channel/history",
            headers={"Authorization": "Bearer test-token"}
        )

    assert response.status_code == 200
    mock_channel.history.assert_called_once_with(20)


def test_channel_history_message_format(client):
    """Channel history returns properly formatted messages."""
    mock_bus = MagicMock()
    mock_channel = MagicMock()
    mock_msg = MagicMock()
    mock_msg.id = 5
    mock_msg.sender = "claude_code"
    mock_msg.content = "Hello"
    mock_msg.timestamp = "2024-01-01T12:00:00"
    mock_msg.reply_to = 3
    mock_channel.history.return_value = [mock_msg]

    with patch("mcp.server.get_event_bus", return_value=mock_bus), \
         patch("mcp.server.get_channel", return_value=mock_channel):
        response = client.get(
            "/channel/history",
            headers={"Authorization": "Bearer test-token"}
        )

    data = response.get_json()
    msg = data["messages"][0]
    assert msg["id"] == 5
    assert msg["from"] == "claude_code"
    assert msg["content"] == "Hello"
    assert msg["timestamp"] == "2024-01-01T12:00:00"
    assert msg["reply_to"] == 3
