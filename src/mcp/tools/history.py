"""Conversation history lookup for Lu.

Read-only access to the conversation messages database written by the
Rust bot at ~/.ludolph/conversations.db. Lets Lu check whether a topic
has come up before so it can build on past answers instead of repeating
itself.
"""

from __future__ import annotations

import sqlite3
from datetime import UTC, datetime, timedelta
from pathlib import Path

DB_PATH = Path.home() / ".ludolph" / "conversations.db"

_PREVIEW_CHARS = 240
_DEFAULT_LIMIT = 10
_MAX_LIMIT = 50
_REPETITION_THRESHOLD = 3


TOOLS = [
    {
        "name": "has_mentioned_before",
        "description": (
            "Search prior conversation for a topic before giving a substantial "
            "answer. CALL THIS FIRST whenever the user asks a non-trivial "
            "question or brings up a topic that might have come up before. "
            "If prior discussion exists, acknowledge it and build on the prior "
            "answer rather than repeating yourself — say 'we talked about this "
            "before' or 'last time I said X, building on that...'. If the "
            "topic has been discussed 3+ times, the tool will surface that so "
            "you can summarize the pattern or ask the user if earlier answers "
            "were helpful. Matches your own past replies by default; pass "
            "include_user_messages=true to also surface what the user said."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": (
                        "The topic, phrase, or question to look up (a few "
                        "words work better than a full sentence)."
                    ),
                },
                "include_user_messages": {
                    "type": "boolean",
                    "description": (
                        "Also match messages from the user, not just your "
                        "own past replies. Default false."
                    ),
                },
                "since_hours": {
                    "type": "integer",
                    "description": "Only return matches from the last N hours.",
                },
                "limit": {
                    "type": "integer",
                    "description": (
                        f"Max entries to show (default {_DEFAULT_LIMIT}, "
                        f"max {_MAX_LIMIT})."
                    ),
                },
            },
            "required": ["topic"],
        },
    },
]


def _connect() -> sqlite3.Connection | None:
    """Open a read-only connection, or None if the DB isn't there."""
    if not DB_PATH.exists():
        return None
    uri = f"file:{DB_PATH}?mode=ro"
    try:
        conn = sqlite3.connect(uri, uri=True, check_same_thread=False)
        conn.row_factory = sqlite3.Row
        return conn
    except sqlite3.Error:
        return None


def _parse_iso(ts: str | None) -> datetime | None:
    if not ts:
        return None
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00"))
    except (ValueError, TypeError):
        return None


def _format_age(dt: datetime | None) -> str:
    if dt is None:
        return "unknown"
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=UTC)
    seconds = int((datetime.now(UTC) - dt).total_seconds())
    if seconds < 0:
        return "just now"
    if seconds < 60:
        return f"{seconds}s ago"
    if seconds < 3600:
        return f"{seconds // 60}m ago"
    if seconds < 86400:
        return f"{seconds // 3600}h ago"
    return f"{seconds // 86400}d ago"


def _preview(text: str, limit: int = _PREVIEW_CHARS) -> str:
    text = " ".join(text.split())
    if len(text) <= limit:
        return text
    return text[: limit - 3] + "..."


def _escape_like(s: str) -> str:
    """Escape SQLite LIKE wildcards so the user's topic matches literally."""
    return s.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")


def _has_mentioned_before(args: dict) -> dict:
    """Search conversation history for prior mentions of a topic."""
    topic = (args.get("topic") or "").strip()
    if not topic:
        return {
            "content": "",
            "error": "topic is required (pass a short phrase to look up).",
        }

    include_user = bool(args.get("include_user_messages"))
    since_hours = args.get("since_hours")
    try:
        limit = int(args.get("limit") or _DEFAULT_LIMIT)
    except (TypeError, ValueError):
        limit = _DEFAULT_LIMIT
    limit = max(1, min(limit, _MAX_LIMIT))

    conn = _connect()
    if conn is None:
        return {
            "content": (
                "No conversation history database found at "
                "~/.ludolph/conversations.db. Either the bot hasn't run yet "
                "on this machine or no messages have been exchanged."
            ),
            "error": None,
        }

    where_clauses = ["LOWER(content) LIKE ? ESCAPE '\\'"]
    params: list = [f"%{_escape_like(topic.lower())}%"]

    if not include_user:
        where_clauses.append("role = 'assistant'")

    if since_hours is not None:
        try:
            hours = int(since_hours)
        except (TypeError, ValueError):
            hours = 0
        if hours > 0:
            cutoff = (datetime.now(UTC) - timedelta(hours=hours)).isoformat()
            where_clauses.append("timestamp >= ?")
            params.append(cutoff)

    where_sql = " AND ".join(where_clauses)

    # Total count (for repetition detection)
    count_sql = f"SELECT COUNT(*) AS n FROM messages WHERE {where_sql}"
    # Top-N most recent
    list_sql = (
        f"SELECT role, content, timestamp FROM messages "
        f"WHERE {where_sql} ORDER BY timestamp DESC LIMIT ?"
    )

    try:
        total = conn.execute(count_sql, params).fetchone()["n"]
        rows = conn.execute(list_sql, [*params, limit]).fetchall()
    except sqlite3.Error as e:
        return {"content": "", "error": f"Failed to query conversation DB: {e}"}
    finally:
        conn.close()

    if total == 0:
        return {
            "content": (
                f"No prior mentions of '{topic}' found. This looks like the "
                "first time it's come up — go ahead and answer fresh."
            ),
            "error": None,
        }

    header = f"Found {total} prior mention(s) of '{topic}':"
    lines = [header, ""]
    for row in rows:
        age = _format_age(_parse_iso(row["timestamp"]))
        lines.append(f"- [{row['role']}] {age}: {_preview(row['content'])}")

    if total >= _REPETITION_THRESHOLD:
        lines.append("")
        lines.append(
            f"Note: you've mentioned this {total} times. Consider summarizing "
            "the prior answers, noting the repetition to the user, or asking "
            "whether earlier answers were helpful."
        )

    return {"content": "\n".join(lines), "error": None}


HANDLERS = {
    "has_mentioned_before": _has_mentioned_before,
}
