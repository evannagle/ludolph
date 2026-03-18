"""Telegram integration tools for Claude Code.

Allows Claude Code to send messages to the Telegram bot and view conversation
history. Useful for debugging, testing, and monitoring bot behavior.

Requires TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID environment variables,
or reads from ~/.ludolph/telegram_token and ~/.ludolph/telegram_chat_id.

Messages are logged to both:
- ~/.ludolph/telegram_history.json (local history)
- .lu/telegram/YYYY-MM-DD.md (vault log, searchable by Claude Code)
"""

import json
import logging
import os
import time
from datetime import datetime
from pathlib import Path
from typing import Any

import requests

from security import get_vault_path

logger = logging.getLogger(__name__)

# Config file paths
CONFIG_DIR = Path.home() / ".ludolph"
TOKEN_FILE = CONFIG_DIR / "telegram_token"
CHAT_ID_FILE = CONFIG_DIR / "telegram_chat_id"
HISTORY_FILE = CONFIG_DIR / "telegram_history.json"

# Vault log directory (relative to vault root)
VAULT_LOG_DIR = ".lu/telegram"

# Telegram API base URL
TELEGRAM_API = "https://api.telegram.org/bot{token}"

# Max history entries to keep
MAX_HISTORY = 100


def _get_bot_token() -> str | None:
    """Get Telegram bot token from env or file."""
    token = os.environ.get("TELEGRAM_BOT_TOKEN")
    if token:
        return token
    if TOKEN_FILE.exists():
        return TOKEN_FILE.read_text().strip()
    return None


def _get_chat_id() -> int | None:
    """Get Telegram chat ID from env or file."""
    chat_id = os.environ.get("TELEGRAM_CHAT_ID")
    if chat_id:
        return int(chat_id)
    if CHAT_ID_FILE.exists():
        return int(CHAT_ID_FILE.read_text().strip())
    return None


def _load_history() -> list[dict]:
    """Load conversation history from file."""
    if not HISTORY_FILE.exists():
        return []
    try:
        with open(HISTORY_FILE) as f:
            return json.load(f)
    except (json.JSONDecodeError, IOError):
        return []


def _save_history(history: list[dict]) -> None:
    """Save conversation history to file."""
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    # Keep only recent entries
    history = history[-MAX_HISTORY:]
    with open(HISTORY_FILE, "w") as f:
        json.dump(history, f, indent=2)


def _log_to_vault(role: str, content: str) -> None:
    """Log a message to the vault for searchability."""
    try:
        vault_path = get_vault_path()
        if not vault_path:
            return

        log_dir = vault_path / VAULT_LOG_DIR
        log_dir.mkdir(parents=True, exist_ok=True)

        # Daily log file
        today = datetime.now().strftime("%Y-%m-%d")
        log_file = log_dir / f"{today}.md"

        timestamp = datetime.now().strftime("%H:%M:%S")
        entry = f"\n### {timestamp} [{role}]\n\n{content}\n"

        # Append to log file
        with open(log_file, "a") as f:
            if not log_file.exists() or log_file.stat().st_size == 0:
                f.write(f"# Telegram Log - {today}\n")
            f.write(entry)

    except Exception as e:
        logger.warning(f"Failed to log to vault: {e}")


def _add_to_history(role: str, content: str, message_id: int | None = None) -> None:
    """Add a message to conversation history."""
    history = _load_history()
    history.append(
        {
            "role": role,
            "content": content,
            "message_id": message_id,
            "timestamp": datetime.now().isoformat(),
        }
    )
    _save_history(history)

    # Also log to vault
    _log_to_vault(role, content)


def _telegram_request(token: str, method: str, **params) -> dict:
    """Make a request to Telegram Bot API."""
    url = f"{TELEGRAM_API.format(token=token)}/{method}"
    response = requests.post(url, json=params, timeout=30)
    response.raise_for_status()
    return response.json()


TOOLS = [
    {
        "name": "telegram_send",
        "description": "Send a message to Telegram as the bot. Use this to test bot responses or send notifications. The message goes to your Telegram chat.",
        "input_schema": {
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message to send",
                },
                "wait_for_response": {
                    "type": "boolean",
                    "description": "Wait for bot's response (up to 30 seconds)",
                    "default": False,
                },
            },
            "required": ["message"],
        },
    },
    {
        "name": "telegram_history",
        "description": "Get recent Telegram conversation history. Shows messages sent and received through this integration.",
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
    {
        "name": "telegram_status",
        "description": "Check Telegram integration status. Verifies bot token and chat ID are configured.",
        "input_schema": {
            "type": "object",
            "properties": {},
        },
    },
]


def _handle_telegram_send(args: dict[str, Any]) -> dict:
    """Send a message to Telegram."""
    message = args.get("message", "").strip()
    wait_for_response = args.get("wait_for_response", False)

    if not message:
        return {"content": "", "error": "Message is required"}

    token = _get_bot_token()
    if not token:
        return {
            "content": "",
            "error": "Telegram bot token not configured. Set TELEGRAM_BOT_TOKEN env var or create ~/.ludolph/telegram_token",
        }

    chat_id = _get_chat_id()
    if not chat_id:
        return {
            "content": "",
            "error": "Telegram chat ID not configured. Set TELEGRAM_CHAT_ID env var or create ~/.ludolph/telegram_chat_id",
        }

    try:
        # Send the message
        result = _telegram_request(
            token,
            "sendMessage",
            chat_id=chat_id,
            text=message,
        )

        if not result.get("ok"):
            return {"content": "", "error": f"Telegram API error: {result}"}

        sent_msg = result.get("result", {})
        message_id = sent_msg.get("message_id")

        # Log to history
        _add_to_history("claude_code", message, message_id)

        response_text = f"Message sent (ID: {message_id})"

        # Optionally wait for response
        if wait_for_response:
            response_text += "\n\nWaiting for bot response..."

            # Get current update offset
            updates = _telegram_request(token, "getUpdates", limit=1, timeout=0)
            offset = 0
            if updates.get("ok") and updates.get("result"):
                offset = updates["result"][-1]["update_id"] + 1

            # Poll for response (up to 30 seconds)
            start_time = time.time()
            while time.time() - start_time < 30:
                updates = _telegram_request(
                    token,
                    "getUpdates",
                    offset=offset,
                    limit=10,
                    timeout=5,
                )

                if updates.get("ok"):
                    for update in updates.get("result", []):
                        offset = update["update_id"] + 1
                        msg = update.get("message", {})
                        # Check if this is from the bot (reply to our message)
                        if msg.get("from", {}).get("is_bot"):
                            bot_text = msg.get("text", "")
                            _add_to_history("bot", bot_text, msg.get("message_id"))
                            return {
                                "content": f"Sent: {message}\n\nBot response:\n{bot_text}",
                                "error": None,
                            }

                time.sleep(1)

            response_text = f"Sent: {message}\n\n(No bot response within 30 seconds - the Pi bot may not be running)"

        return {"content": response_text, "error": None}

    except requests.RequestException as e:
        return {"content": "", "error": f"Request failed: {e}"}


def _handle_telegram_history(args: dict[str, Any]) -> dict:
    """Get recent conversation history."""
    limit = args.get("limit", 20)
    history = _load_history()

    if not history:
        return {"content": "No conversation history found.", "error": None}

    # Get recent entries
    recent = history[-limit:]

    lines = ["Recent Telegram messages:\n"]
    for entry in recent:
        ts = entry.get("timestamp", "")[:19]  # Trim microseconds
        role = entry.get("role", "unknown")
        content = entry.get("content", "")[:200]  # Truncate long messages
        if len(entry.get("content", "")) > 200:
            content += "..."
        lines.append(f"[{ts}] {role}: {content}")

    return {"content": "\n".join(lines), "error": None}


def _handle_telegram_status(args: dict[str, Any]) -> dict:
    """Check Telegram integration status."""
    token = _get_bot_token()
    chat_id = _get_chat_id()

    status_lines = ["Telegram Integration Status\n"]

    # Check token
    if token:
        status_lines.append(f"Bot token: configured ({token[:10]}...)")
        # Try to get bot info
        try:
            result = _telegram_request(token, "getMe")
            if result.get("ok"):
                bot = result.get("result", {})
                status_lines.append(f"Bot name: @{bot.get('username', 'unknown')}")
        except Exception as e:
            status_lines.append(f"Bot verification failed: {e}")
    else:
        status_lines.append("Bot token: NOT CONFIGURED")
        status_lines.append("  Set TELEGRAM_BOT_TOKEN or create ~/.ludolph/telegram_token")

    # Check chat ID
    if chat_id:
        status_lines.append(f"Chat ID: {chat_id}")
    else:
        status_lines.append("Chat ID: NOT CONFIGURED")
        status_lines.append("  Set TELEGRAM_CHAT_ID or create ~/.ludolph/telegram_chat_id")

    # Check history file
    history = _load_history()
    status_lines.append(f"History entries: {len(history)}")

    # Overall status
    if token and chat_id:
        status_lines.append("\nStatus: Ready to send messages")
    else:
        status_lines.append("\nStatus: Configuration incomplete")

    return {"content": "\n".join(status_lines), "error": None}


HANDLERS = {
    "telegram_send": _handle_telegram_send,
    "telegram_history": _handle_telegram_history,
    "telegram_status": _handle_telegram_status,
}
