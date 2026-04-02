"""Persistent observation store for long-term user knowledge.

Stores facts, preferences, and contextual notes about users that
persist across conversations. Backed by SQLite with FTS5 search.

Designed for future migration to LanceDB (vector store) — the
ObservationStore protocol ensures tool handlers stay unchanged.
"""

import sqlite3
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

DB_PATH = Path.home() / ".ludolph" / "observations.db"


class SqliteObservationStore:
    """SQLite-backed observation store with full-text search."""

    def __init__(self, db_path: Path = DB_PATH):
        db_path.parent.mkdir(parents=True, exist_ok=True)
        self.conn = sqlite3.connect(str(db_path), check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        self._init_schema()

    def _init_schema(self) -> None:
        self.conn.executescript("""
            CREATE TABLE IF NOT EXISTS observations (
                id TEXT PRIMARY KEY,
                user_id INTEGER NOT NULL,
                text TEXT NOT NULL,
                title TEXT,
                category TEXT NOT NULL DEFAULT 'fact',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                source TEXT DEFAULT 'tool'
            );

            CREATE INDEX IF NOT EXISTS idx_obs_user_cat
                ON observations(user_id, category, created_at DESC);

            CREATE VIRTUAL TABLE IF NOT EXISTS observations_fts USING fts5(
                text, title, category,
                content='observations',
                content_rowid='rowid'
            );

            CREATE TRIGGER IF NOT EXISTS observations_ai AFTER INSERT ON observations BEGIN
                INSERT INTO observations_fts(rowid, text, title, category)
                VALUES (new.rowid, new.text, new.title, new.category);
            END;

            CREATE TRIGGER IF NOT EXISTS observations_ad AFTER DELETE ON observations BEGIN
                INSERT INTO observations_fts(observations_fts, rowid, text, title, category)
                VALUES ('delete', old.rowid, old.text, old.title, old.category);
            END;

            CREATE TRIGGER IF NOT EXISTS observations_au AFTER UPDATE ON observations BEGIN
                INSERT INTO observations_fts(observations_fts, rowid, text, title, category)
                VALUES ('delete', old.rowid, old.text, old.title, old.category);
                INSERT INTO observations_fts(rowid, text, title, category)
                VALUES (new.rowid, new.text, new.title, new.category);
            END;
        """)

    def save(
        self,
        user_id: int,
        text: str,
        category: str = "fact",
        title: str | None = None,
        source: str = "tool",
    ) -> dict:
        """Save an observation. Returns the created record."""
        obs_id = str(uuid.uuid4())
        now = datetime.now(timezone.utc).isoformat()

        self.conn.execute(
            """INSERT INTO observations (id, user_id, text, title, category, created_at, updated_at, source)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
            (obs_id, user_id, text, title, category, now, now, source),
        )
        self.conn.commit()

        return {
            "id": obs_id,
            "title": title,
            "category": category,
            "created_at": now,
        }

    def search(
        self,
        user_id: int,
        query: str,
        category: str | None = None,
        limit: int = 10,
    ) -> list[dict]:
        """Search observations using FTS5. Returns ranked results."""
        if category:
            rows = self.conn.execute(
                """SELECT o.id, o.title, o.text, o.category, o.created_at
                   FROM observations o
                   JOIN observations_fts f ON o.rowid = f.rowid
                   WHERE observations_fts MATCH ? AND o.user_id = ? AND o.category = ?
                   ORDER BY rank
                   LIMIT ?""",
                (query, user_id, category, limit),
            ).fetchall()
        else:
            rows = self.conn.execute(
                """SELECT o.id, o.title, o.text, o.category, o.created_at
                   FROM observations o
                   JOIN observations_fts f ON o.rowid = f.rowid
                   WHERE observations_fts MATCH ? AND o.user_id = ?
                   ORDER BY rank
                   LIMIT ?""",
                (query, user_id, limit),
            ).fetchall()

        return [dict(row) for row in rows]

    def get(self, ids: list[str]) -> list[dict]:
        """Get observations by ID."""
        if not ids:
            return []

        placeholders = ",".join("?" for _ in ids)
        rows = self.conn.execute(
            f"""SELECT id, user_id, text, title, category, created_at, updated_at, source
                FROM observations WHERE id IN ({placeholders})""",
            ids,
        ).fetchall()

        return [dict(row) for row in rows]

    def delete(self, obs_id: str, user_id: int) -> bool:
        """Delete an observation. Returns True if deleted."""
        cursor = self.conn.execute(
            "DELETE FROM observations WHERE id = ? AND user_id = ?",
            (obs_id, user_id),
        )
        self.conn.commit()
        return cursor.rowcount > 0

    def recent(self, user_id: int, limit: int = 20) -> list[dict]:
        """Get most recent observations for a user (for system prompt injection)."""
        rows = self.conn.execute(
            """SELECT id, title, text, category, created_at
               FROM observations
               WHERE user_id = ?
               ORDER BY updated_at DESC
               LIMIT ?""",
            (user_id, limit),
        ).fetchall()

        return [dict(row) for row in rows]

    def list_all(self, user_id: int, category: str | None = None, limit: int = 50) -> list[dict]:
        """List observations for a user, optionally filtered by category."""
        if category:
            rows = self.conn.execute(
                """SELECT id, title, text, category, created_at
                   FROM observations WHERE user_id = ? AND category = ?
                   ORDER BY updated_at DESC LIMIT ?""",
                (user_id, category, limit),
            ).fetchall()
        else:
            rows = self.conn.execute(
                """SELECT id, title, text, category, created_at
                   FROM observations WHERE user_id = ?
                   ORDER BY updated_at DESC LIMIT ?""",
                (user_id, limit),
            ).fetchall()

        return [dict(row) for row in rows]


# Module-level store instance
_store = SqliteObservationStore()


def get_recent_observations(user_id: int, limit: int = 20) -> list[dict]:
    """Public accessor for the /observations/recent endpoint."""
    return _store.recent(user_id, limit)


# --- MCP Tool Handlers ---


def _save_observation(args: dict[str, Any]) -> dict:
    text = args.get("text", "").strip()
    if not text:
        return {"content": "", "error": "Observation text is required"}

    user_id = args.get("user_id", 0)
    if not user_id:
        return {"content": "", "error": "user_id is required"}

    category = args.get("category", "fact")
    if category not in ("preference", "fact", "context"):
        return {"content": "", "error": f"Invalid category: {category}. Use preference, fact, or context."}

    title = args.get("title")
    result = _store.save(user_id, text, category, title)

    return {"content": f"Saved {category}: {title or text[:60]}"}


def _search_observations(args: dict[str, Any]) -> dict:
    query = args.get("query", "").strip()
    if not query:
        return {"content": "", "error": "Search query is required"}

    user_id = args.get("user_id", 0)
    if not user_id:
        return {"content": "", "error": "user_id is required"}

    category = args.get("category")
    limit = args.get("limit", 10)

    results = _store.search(user_id, query, category, limit)

    if not results:
        return {"content": "No observations found."}

    lines = []
    for obs in results:
        tag = f"[{obs['category']}]"
        title = obs.get("title") or ""
        if title:
            lines.append(f"- {tag} {title}: {obs['text']} (id: {obs['id'][:8]})")
        else:
            lines.append(f"- {tag} {obs['text']} (id: {obs['id'][:8]})")

    return {"content": f"Found {len(results)} observation(s):\n" + "\n".join(lines)}


def _get_observations(args: dict[str, Any]) -> dict:
    ids = args.get("ids", [])
    if not ids:
        return {"content": "", "error": "At least one ID is required"}

    results = _store.get(ids)

    if not results:
        return {"content": "No observations found for the given IDs."}

    lines = []
    for obs in results:
        title = obs.get("title") or "(untitled)"
        lines.append(
            f"[{obs['category']}] {title}\n"
            f"  {obs['text']}\n"
            f"  Created: {obs['created_at']} | ID: {obs['id']}"
        )

    return {"content": "\n\n".join(lines)}


def _delete_observation(args: dict[str, Any]) -> dict:
    obs_id = args.get("id", "").strip()
    if not obs_id:
        return {"content": "", "error": "Observation ID is required"}

    user_id = args.get("user_id", 0)
    if not user_id:
        return {"content": "", "error": "user_id is required"}

    deleted = _store.delete(obs_id, user_id)

    if deleted:
        return {"content": f"Deleted observation {obs_id[:8]}."}
    return {"content": "", "error": f"Observation {obs_id[:8]} not found or not owned by you."}


# --- Tool Definitions ---

TOOLS = [
    {
        "name": "save_observation",
        "description": (
            "Save a fact, preference, or contextual note about the user for long-term recall. "
            "Use this proactively when the user reveals preferences, personal facts, or "
            "contextual knowledge you should remember across conversations."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The observation text. Be specific and self-contained.",
                },
                "title": {
                    "type": "string",
                    "description": "Optional short label (e.g., 'Default hitlist location')",
                },
                "category": {
                    "type": "string",
                    "enum": ["preference", "fact", "context"],
                    "description": (
                        "preference = user preferences/defaults/settings, "
                        "fact = biographical facts about the user, "
                        "context = situational knowledge (projects, goals, deadlines)"
                    ),
                },
                "user_id": {
                    "type": "integer",
                    "description": "Telegram user ID",
                },
            },
            "required": ["text", "category", "user_id"],
        },
    },
    {
        "name": "search_observations",
        "description": (
            "Search stored observations about the user. Returns matching facts, "
            "preferences, and context notes ranked by relevance."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (full-text search)",
                },
                "category": {
                    "type": "string",
                    "enum": ["preference", "fact", "context"],
                    "description": "Filter by category (optional)",
                },
                "user_id": {
                    "type": "integer",
                    "description": "Telegram user ID",
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results (default 10)",
                },
            },
            "required": ["query", "user_id"],
        },
    },
    {
        "name": "get_observations",
        "description": "Get full details of specific observations by ID.",
        "input_schema": {
            "type": "object",
            "properties": {
                "ids": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "List of observation UUIDs to retrieve",
                },
            },
            "required": ["ids"],
        },
    },
    {
        "name": "delete_observation",
        "description": "Delete an observation that is no longer accurate or relevant.",
        "input_schema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "UUID of the observation to delete",
                },
                "user_id": {
                    "type": "integer",
                    "description": "Telegram user ID (for ownership verification)",
                },
            },
            "required": ["id", "user_id"],
        },
    },
]

HANDLERS = {
    "save_observation": _save_observation,
    "search_observations": _search_observations,
    "get_observations": _get_observations,
    "delete_observation": _delete_observation,
}
