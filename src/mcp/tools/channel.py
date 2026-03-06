"""MCP tools for channel messaging.

Allows Claude Code to send messages to Lu and view conversation history.
"""

import logging
import time
from typing import Any

from ..channel import get_channel
from ..event_bus import get_event_bus
from ..security import get_vault_path

logger = logging.getLogger(__name__)

TOOLS = [
    {
        "name": "channel_send",
        "description": "Send a message to Lu and optionally wait for response. Set wait_for_response=true to get Lu's reply in the same call.",
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
                "wait_for_response": {
                    "type": "boolean",
                    "description": "Wait for Lu's response (default: true, max 60s)",
                    "default": True,
                },
                "timeout": {
                    "type": "integer",
                    "description": "Max seconds to wait for response (default: 60)",
                    "default": 60,
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
    """Send a message to the channel, optionally waiting for response."""
    content = args.get("content", "").strip()
    reply_to = args.get("reply_to")
    wait_for_response = args.get("wait_for_response", True)
    timeout = args.get("timeout", 60)

    if not content:
        return {"content": "", "error": "Message content is required"}

    try:
        bus = get_event_bus()
        channel = get_channel(bus, get_vault_path())

        msg = channel.send("claude_code", content, reply_to)
        sent_id = msg.id

        if not wait_for_response:
            return {
                "content": f"Message sent (ID: {sent_id}). Lu will respond shortly.",
                "error": None,
            }

        # Wait for Lu's response (poll every 2 seconds)
        start_time = time.time()
        while time.time() - start_time < timeout:
            time.sleep(2)

            # Check for new messages from Lu
            history = channel.history(10)
            for m in reversed(history):
                # Look for Lu's reply to our message
                if m.sender == "lu" and m.reply_to == sent_id:
                    return {
                        "content": f"Lu's response:\n\n{m.content}",
                        "error": None,
                        "message_id": m.id,
                        "sent_id": sent_id,
                    }

        return {
            "content": f"Message sent (ID: {sent_id}) but no response within {timeout}s. Lu may still be processing.",
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
