"""Conversation memory tools for long-term storage.

Provides claude-mem inspired endpoints for persisting conversations
to vault files in .lu/conversations/ directory.

Supports session-scoped search to filter by conversation context.
"""

import re
from datetime import datetime, timedelta
from pathlib import Path

from security import safe_path

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
                "session_id": {
                    "type": "string",
                    "description": "Optional session identifier for grouping related conversations",
                },
            },
            "required": ["messages", "user_id"],
        },
    },
    {
        "name": "search_conversations",
        "description": "Search past conversations by content. Returns matching excerpts with dates. Supports scoping to filter by context.",
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
                "scope": {
                    "type": "string",
                    "description": "Search scope: 'all' (default), 'session' (current session), 'recent' (last 7 days)",
                    "enum": ["all", "session", "recent"],
                },
                "session_id": {
                    "type": "string",
                    "description": "Session ID to filter by (required if scope='session')",
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


def _format_message(msg: dict, session_id: str | None = None) -> str:
    """Format a single message for markdown storage.

    If session_id is provided, includes it as metadata.
    """
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

    # Include session metadata if provided
    session_tag = f" [session:{session_id}]" if session_id else ""
    return f"### {time_str}{session_tag}\n**{role_label}**: {content}\n"


def _save_conversation(args: dict) -> dict:
    """Save conversation messages to vault with optional session tracking."""
    messages = args.get("messages", [])
    user_id = args.get("user_id", 0)
    session_id = args.get("session_id")

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
            content_parts.append(_format_message(msg, session_id))

        content_parts.append("---\n")
        content = "\n".join(content_parts)

        # Append to file
        with file_path.open("a", encoding="utf-8") as f:
            f.write(content)

        files_written.append(date_str)

    session_info = f" (session: {session_id})" if session_id else ""
    return {
        "content": f"Saved {len(messages)} messages to {len(files_written)} file(s): {', '.join(files_written)}{session_info}",
        "error": None,
    }


def _search_conversations(args: dict) -> dict:
    """Search past conversations with optional scope filtering.

    Scope options:
    - 'all': Search all conversations (default)
    - 'session': Filter by session_id
    - 'recent': Only search last 7 days
    """
    query = args.get("query", "").lower()
    limit = args.get("limit", 10)
    scope = args.get("scope", "all")
    session_id = args.get("session_id")

    if not query:
        return {"content": "", "error": "Query is required"}

    if scope == "session" and not session_id:
        return {"content": "", "error": "session_id is required when scope='session'"}

    conv_dir = _get_conversations_dir()
    if not conv_dir or not conv_dir.exists():
        return {"content": "No conversation history found", "error": None}

    # Calculate date cutoff for 'recent' scope
    recent_cutoff = None
    if scope == "recent":
        recent_cutoff = datetime.now() - timedelta(days=7)

    results = []

    # Search markdown files (sorted by date, newest first)
    for file_path in sorted(conv_dir.glob("*.md"), reverse=True):
        if len(results) >= limit:
            break

        # Apply date filtering for 'recent' scope
        if scope == "recent" and recent_cutoff:
            try:
                file_date = datetime.strptime(file_path.stem, "%Y-%m-%d")
                if file_date < recent_cutoff:
                    continue
            except ValueError:
                continue

        content = file_path.read_text(encoding="utf-8")

        # Find matching sections
        sections = content.split("---")
        for section in sections:
            # Apply session filtering
            if scope == "session" and session_id:
                if f"[session:{session_id}]" not in section:
                    continue

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
        scope_info = ""
        if scope == "session":
            scope_info = f" in session '{session_id}'"
        elif scope == "recent":
            scope_info = " in the last 7 days"
        return {"content": f"No conversations found matching '{query}'{scope_info}", "error": None}

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
        "content": f"Conversation history available for {len(dates)} date(s):\n"
        + "\n".join(f"- {d}" for d in dates),
        "error": None,
    }


HANDLERS = {
    "save_conversation": _save_conversation,
    "search_conversations": _search_conversations,
    "get_conversation": _get_conversation,
    "list_conversation_dates": _list_conversation_dates,
}
