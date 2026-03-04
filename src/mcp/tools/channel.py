"""MCP tools for channel messaging.

Allows Claude Code to send messages to Lu and view conversation history.
"""

import logging
from typing import Any

from ..channel import get_channel
from ..event_bus import get_event_bus
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
            direction = "-> lu" if msg.sender != "lu" else "<- lu"
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
