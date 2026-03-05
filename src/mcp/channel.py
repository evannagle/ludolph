"""Channel messaging between Claude Code and Lu.

Wraps the Event Bus to provide higher-level messaging with:
- ChannelMessage storage for conversation history
- Vault logging to .lu/channel/YYYY-MM-DD.md for searchability
"""

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
    context: dict | None = None  # Git context: {repo, branch, recent_commits}


class Channel:
    """Bidirectional message channel with vault logging.

    Wraps an EventBus to provide message storage and vault persistence.
    """

    def __init__(
        self,
        event_bus: "EventBus",
        vault_path: Path | None = None,
    ):
        """Initialize the channel.

        Args:
            event_bus: The event bus to publish messages to.
            vault_path: Optional path to vault for logging messages.
        """
        self._bus = event_bus
        self._vault_path = vault_path
        self._messages: deque[ChannelMessage] = deque(maxlen=MAX_MESSAGES)
        self._next_id = 1
        self._lock = threading.Lock()

    def send(
        self,
        sender: str,
        content: str,
        reply_to: int | None = None,
        context: dict | None = None,
    ) -> ChannelMessage:
        """Send a message to the channel.

        Args:
            sender: The sender identifier (e.g., "claude_code", "lu").
            content: The message content.
            reply_to: Optional ID of message being replied to.
            context: Optional git context (repo, branch, recent_commits).

        Returns:
            The created ChannelMessage.
        """
        with self._lock:
            msg = ChannelMessage(
                id=self._next_id,
                sender=sender,
                content=content,
                timestamp=datetime.now().isoformat(),
                reply_to=reply_to,
                context=context,
            )
            self._next_id += 1
            self._messages.append(msg)

        # Publish event to bus
        event_data = {
            "id": msg.id,
            "from": msg.sender,
            "content": msg.content,
            "reply_to": msg.reply_to,
        }
        if context:
            event_data["context"] = context

        self._bus.publish(
            "channel_message",
            event_data,
            source=sender,
        )

        # Log to vault
        self._log_to_vault(msg)

        return msg

    def history(self, limit: int = 20) -> list[ChannelMessage]:
        """Get recent message history.

        Args:
            limit: Maximum number of messages to return.

        Returns:
            List of recent messages, oldest first.
        """
        with self._lock:
            return list(self._messages)[-limit:]

    def _log_to_vault(self, msg: ChannelMessage) -> None:
        """Log message to vault for searchability.

        Messages are logged to .lu/channel/YYYY-MM-DD.md files.
        """
        if not self._vault_path:
            return

        try:
            log_dir = self._vault_path / CHANNEL_LOG_DIR
            log_dir.mkdir(parents=True, exist_ok=True)

            today = datetime.now().strftime("%Y-%m-%d")
            log_file = log_dir / f"{today}.md"

            # Extract time portion from ISO timestamp
            timestamp = msg.timestamp[11:19]

            # Determine direction arrow
            if msg.sender == "lu":
                direction = "lu → claude_code"
            else:
                direction = f"{msg.sender} → lu"

            entry = f"\n### {timestamp} [{direction}]\n\n{msg.content}\n"

            # Check if file exists and has content before writing header
            needs_header = not log_file.exists() or log_file.stat().st_size == 0

            with open(log_file, "a") as f:
                if needs_header:
                    f.write(f"# Channel Log - {today}\n")
                f.write(entry)

        except Exception as e:
            logger.warning(f"Failed to log channel message: {e}")


# Global instance
_channel: Channel | None = None


def get_channel(event_bus: "EventBus", vault_path: Path | None = None) -> Channel:
    """Get or create the global channel.

    Args:
        event_bus: The event bus to use.
        vault_path: Optional vault path for logging.

    Returns:
        The global Channel instance.
    """
    global _channel
    if _channel is None:
        _channel = Channel(event_bus, vault_path)
    return _channel


def reset_channel() -> None:
    """Reset the global channel instance. For testing only."""
    global _channel
    _channel = None
