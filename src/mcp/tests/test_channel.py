"""Tests for channel messaging."""

import pytest
from unittest.mock import MagicMock
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


def test_send_increments_message_id():
    """Each message gets a unique incrementing ID."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=None)

    msg1 = channel.send("cc", "first")
    msg2 = channel.send("lu", "second")
    msg3 = channel.send("cc", "third")

    assert msg1.id == 1
    assert msg2.id == 2
    assert msg3.id == 3


def test_send_has_timestamp():
    """Messages have ISO format timestamps."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=None)

    msg = channel.send("cc", "test")

    assert msg.timestamp is not None
    assert "T" in msg.timestamp  # ISO format


def test_send_publishes_event_with_correct_data():
    """Send publishes event with message details."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=None)

    channel.send("claude_code", "Hello Lu", reply_to=5)

    mock_bus.publish.assert_called_once_with(
        "channel_message",
        {
            "id": 1,
            "from": "claude_code",
            "content": "Hello Lu",
            "reply_to": 5,
        },
        source="claude_code",
    )


def test_send_with_reply_to():
    """Messages can reference a previous message."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=None)

    msg1 = channel.send("cc", "original")
    msg2 = channel.send("lu", "reply", reply_to=msg1.id)

    assert msg2.reply_to == 1


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


def test_history_returns_all_when_fewer_than_limit():
    """History returns all messages when count is less than limit."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=None)

    channel.send("cc", "only one")

    history = channel.history(limit=10)

    assert len(history) == 1
    assert history[0].content == "only one"


def test_history_empty_when_no_messages():
    """History returns empty list when no messages."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=None)

    history = channel.history()

    assert history == []


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


def test_vault_log_creates_directory(tmp_path):
    """Vault logging creates .lu/channel directory if needed."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=tmp_path)

    channel.send("cc", "test")

    assert (tmp_path / ".lu" / "channel").is_dir()


def test_vault_log_has_header(tmp_path):
    """Vault log file has date header."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=tmp_path)

    channel.send("cc", "test")

    log_files = list((tmp_path / ".lu" / "channel").glob("*.md"))
    content = log_files[0].read_text()

    assert content.startswith("# Channel Log -")


def test_vault_log_shows_direction_from_lu(tmp_path):
    """Lu messages show direction arrow to claude_code."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=tmp_path)

    channel.send("lu", "outgoing message")

    log_files = list((tmp_path / ".lu" / "channel").glob("*.md"))
    content = log_files[0].read_text()

    assert "lu → claude_code" in content


def test_vault_log_shows_direction_to_lu(tmp_path):
    """Non-lu messages show direction arrow to lu."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=tmp_path)

    channel.send("claude_code", "incoming message")

    log_files = list((tmp_path / ".lu" / "channel").glob("*.md"))
    content = log_files[0].read_text()

    assert "claude_code → lu" in content


def test_no_vault_logging_when_path_is_none():
    """No error when vault_path is None."""
    mock_bus = MagicMock()
    channel = Channel(event_bus=mock_bus, vault_path=None)

    # Should not raise
    msg = channel.send("cc", "test")

    assert msg.content == "test"


def test_channel_message_dataclass():
    """ChannelMessage is a proper dataclass."""
    msg = ChannelMessage(
        id=1,
        sender="cc",
        content="hello",
        timestamp="2024-01-01T12:00:00",
        reply_to=None,
    )

    assert msg.id == 1
    assert msg.sender == "cc"
    assert msg.content == "hello"
    assert msg.timestamp == "2024-01-01T12:00:00"
    assert msg.reply_to is None


def test_channel_message_with_reply_to():
    """ChannelMessage can have reply_to."""
    msg = ChannelMessage(
        id=2,
        sender="lu",
        content="reply",
        timestamp="2024-01-01T12:00:01",
        reply_to=1,
    )

    assert msg.reply_to == 1
