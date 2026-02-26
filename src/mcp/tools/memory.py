"""Conversation memory tools for long-term storage.

Provides claude-mem inspired endpoints for persisting conversations
to vault files in .lu/conversations/ directory.
"""

import re
from datetime import datetime
from pathlib import Path

from ..security import safe_path

# Conversation storage directory (relative to vault root)
CONVERSATIONS_DIR = ".lu/conversations"

TOOLS = [
    {
        "name": "save_conversation",
        "description": "Save conversation messages to long-term vault storage. Called by Pi client when short-term memory overflows.",
        "input_schema": {
            "type": "object",
            "properties": {
                "messages": {
                    "type": "array",
                    "description": "Array of message objects with role, content, and timestamp",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": {"type": "string", "enum": ["user", "assistant"]},
                            "content": {"type": "string"},
                            "timestamp": {"type": "string", "description": "ISO 8601 timestamp"},
                        },
                        "required": ["role", "content", "timestamp"],
                    },
                },
                "user_id": {
                    "type": "integer",
                    "description": "Telegram user ID for organizing conversations",
                },
            },
            "required": ["messages", "user_id"],
        },
    },
    {
        "name": "search_conversations",
        "description": "Search past conversations by content. Returns matching excerpts with dates.",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (case-insensitive)",
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results to return (default 10)",
                },
            },
            "required": ["query"],
        },
    },
    {
        "name": "get_conversation",
        "description": "Retrieve conversation history for a specific date.",
        "input_schema": {
            "type": "object",
            "properties": {
                "date": {
                    "type": "string",
                    "description": "Date in YYYY-MM-DD format",
                },
            },
            "required": ["date"],
        },
    },
    {
        "name": "list_conversation_dates",
        "description": "List all dates that have conversation history.",
        "input_schema": {
            "type": "object",
            "properties": {},
        },
    },
]


def _get_conversations_dir() -> Path:
    """Get the conversations directory, creating if needed."""
    path = safe_path(CONVERSATIONS_DIR)
    if path:
        path.mkdir(parents=True, exist_ok=True)
    return path


def _format_message(msg: dict) -> str:
    """Format a single message for markdown storage."""
    role = msg.get("role", "user")
    content = msg.get("content", "")
    timestamp = msg.get("timestamp", "")

    # Parse timestamp to get time
    try:
        dt = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
        time_str = dt.strftime("%I:%M %p")
    except (ValueError, AttributeError):
        time_str = "??:??"

    role_label = "User" if role == "user" else "Lu"
    return f"### {time_str}\n**{role_label}**: {content}\n"


def _save_conversation(args: dict) -> dict:
    """Save conversation messages to vault."""
    messages = args.get("messages", [])
    user_id = args.get("user_id", 0)

    if not messages:
        return {"content": "No messages to save", "error": None}

    conv_dir = _get_conversations_dir()
    if not conv_dir:
        return {"content": "", "error": "Invalid conversations directory"}

    # Group messages by date
    by_date: dict[str, list] = {}
    for msg in messages:
        timestamp = msg.get("timestamp", "")
        try:
            dt = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
            date_str = dt.strftime("%Y-%m-%d")
        except (ValueError, AttributeError):
            date_str = datetime.now().strftime("%Y-%m-%d")

        if date_str not in by_date:
            by_date[date_str] = []
        by_date[date_str].append(msg)

    # Write to files
    files_written = []
    for date_str, date_messages in by_date.items():
        file_path = conv_dir / f"{date_str}.md"

        # Build content
        content_parts = []

        # Add date header if file is new
        if not file_path.exists():
            content_parts.append(f"## {date_str}\n")

        for msg in date_messages:
            content_parts.append(_format_message(msg))

        content_parts.append("---\n")
        content = "\n".join(content_parts)

        # Append to file
        with file_path.open("a", encoding="utf-8") as f:
            f.write(content)

        files_written.append(date_str)

    return {
        "content": f"Saved {len(messages)} messages to {len(files_written)} file(s): {', '.join(files_written)}",
        "error": None,
    }


def _search_conversations(args: dict) -> dict:
    """Search past conversations."""
    query = args.get("query", "").lower()
    limit = args.get("limit", 10)

    if not query:
        return {"content": "", "error": "Query is required"}

    conv_dir = _get_conversations_dir()
    if not conv_dir or not conv_dir.exists():
        return {"content": "No conversation history found", "error": None}

    results = []

    # Search all markdown files
    for file_path in sorted(conv_dir.glob("*.md"), reverse=True):
        if len(results) >= limit:
            break

        content = file_path.read_text(encoding="utf-8")

        # Find matching sections
        sections = content.split("---")
        for section in sections:
            if query in section.lower():
                # Extract date from filename
                date_str = file_path.stem

                # Clean up section for display
                excerpt = section.strip()[:500]
                if len(section.strip()) > 500:
                    excerpt += "..."

                results.append(f"**{date_str}**:\n{excerpt}")

                if len(results) >= limit:
                    break

    if not results:
        return {"content": f"No conversations found matching '{query}'", "error": None}

    return {
        "content": f"Found {len(results)} result(s):\n\n" + "\n\n---\n\n".join(results),
        "error": None,
    }


def _get_conversation(args: dict) -> dict:
    """Get conversation for a specific date."""
    date_str = args.get("date", "")

    # Validate date format
    if not re.match(r"^\d{4}-\d{2}-\d{2}$", date_str):
        return {"content": "", "error": "Invalid date format. Use YYYY-MM-DD"}

    conv_dir = _get_conversations_dir()
    if not conv_dir:
        return {"content": "", "error": "Invalid conversations directory"}

    file_path = conv_dir / f"{date_str}.md"

    if not file_path.exists():
        return {"content": f"No conversation found for {date_str}", "error": None}

    content = file_path.read_text(encoding="utf-8")
    return {"content": content, "error": None}


def _list_conversation_dates(args: dict) -> dict:
    """List all dates with conversation history."""
    conv_dir = _get_conversations_dir()
    if not conv_dir or not conv_dir.exists():
        return {"content": "No conversation history found", "error": None}

    dates = sorted(
        [f.stem for f in conv_dir.glob("*.md")],
        reverse=True,
    )

    if not dates:
        return {"content": "No conversation history found", "error": None}

    return {
        "content": f"Conversation history available for {len(dates)} date(s):\n" + "\n".join(f"- {d}" for d in dates),
        "error": None,
    }


HANDLERS = {
    "save_conversation": _save_conversation,
    "search_conversations": _search_conversations,
    "get_conversation": _get_conversation,
    "list_conversation_dates": _list_conversation_dates,
}
