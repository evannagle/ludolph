"""Schedule execution visibility for Lu.

Read-only access to the scheduler database written by the Rust bot at
~/.ludolph/schedules.db. Lets Lu answer "how are my schedules doing?"
and surface recent failures without waiting for the user to ask.
"""

from __future__ import annotations

import sqlite3
from datetime import UTC, datetime, timedelta
from pathlib import Path

DB_PATH = Path.home() / ".ludolph" / "schedules.db"

# Keep result payloads small — Lu only needs a preview.
_RESULT_PREVIEW_CHARS = 200
_ERROR_PREVIEW_CHARS = 200
_DEFAULT_RUN_LIMIT = 20
_MAX_RUN_LIMIT = 200


TOOLS = [
    {
        "name": "list_schedule_runs",
        "description": (
            "List recent scheduled-task executions (successes and failures) from "
            "the Ludolph scheduler. Use when the user asks how a schedule went, "
            "which runs failed, or what has been running. Each entry includes the "
            "schedule name, status, when it started, duration, a result preview, "
            "and any error message. Filters are optional — with no filters you "
            "get the most recent executions across all schedules."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "schedule_name": {
                    "type": "string",
                    "description": "Filter to a schedule whose name contains this text (case-insensitive).",
                },
                "status": {
                    "type": "string",
                    "enum": ["running", "success", "error", "cancelled"],
                    "description": "Only return runs with this status.",
                },
                "since_hours": {
                    "type": "integer",
                    "description": "Only return runs started within the last N hours.",
                },
                "limit": {
                    "type": "integer",
                    "description": f"Max entries to return (default {_DEFAULT_RUN_LIMIT}, max {_MAX_RUN_LIMIT}).",
                },
            },
            "required": [],
        },
    },
    {
        "name": "schedule_health",
        "description": (
            "Snapshot of scheduled-task health: recent failures (last 24h and 7d), "
            "schedules that are active but haven't run when they should have, and "
            "overall run counts. Use this proactively when the user asks about "
            "status, what's running, or whether anything is broken. Surface "
            "failures by name so the user can act on them."
        ),
        "input_schema": {
            "type": "object",
            "properties": {},
            "required": [],
        },
    },
]


_SCHEMA_SQL = """
CREATE TABLE IF NOT EXISTS schedules (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    prompt TEXT NOT NULL,
    cron_expression TEXT NOT NULL,
    timezone TEXT DEFAULT 'local',
    next_run TEXT,
    status TEXT DEFAULT 'active',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_run TEXT,
    last_result TEXT,
    notify_before INTEGER DEFAULT 0,
    notify_after INTEGER DEFAULT 1,
    tags TEXT,
    priority INTEGER DEFAULT 0,
    run_count INTEGER DEFAULT 0,
    max_runs INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_schedules_user ON schedules(user_id);
CREATE INDEX IF NOT EXISTS idx_schedules_status ON schedules(status);
CREATE INDEX IF NOT EXISTS idx_schedules_next_run ON schedules(next_run);

CREATE TABLE IF NOT EXISTS schedule_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    schedule_id TEXT NOT NULL,
    user_id INTEGER NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    status TEXT NOT NULL,
    result_summary TEXT,
    error_message TEXT,
    FOREIGN KEY (schedule_id) REFERENCES schedules(id)
);

CREATE INDEX IF NOT EXISTS idx_runs_schedule ON schedule_runs(schedule_id);
CREATE INDEX IF NOT EXISTS idx_runs_user ON schedule_runs(user_id);
"""


def _ensure_schema() -> None:
    """Create the scheduler schema if the DB doesn't exist or is empty.

    This lets the MCP tool work without requiring the Rust scheduler to have
    run first. The Pi's scheduler pushes runs via /schedule_runs/record.
    """
    DB_PATH.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(DB_PATH), check_same_thread=False)
    try:
        conn.executescript(_SCHEMA_SQL)
        conn.commit()
    finally:
        conn.close()


def _connect() -> sqlite3.Connection | None:
    """Open a read-only connection to the scheduler DB, or None if missing."""
    # Ensure schema exists (idempotent, handles fresh Mac installs)
    try:
        _ensure_schema()
    except sqlite3.Error:
        return None

    # Read-only URI so we never contend with the Rust writer.
    uri = f"file:{DB_PATH}?mode=ro"
    try:
        conn = sqlite3.connect(uri, uri=True, check_same_thread=False)
        conn.row_factory = sqlite3.Row
        return conn
    except sqlite3.Error:
        return None


def record_run(schedule_id: str, schedule_name: str, user_id: int, status: str,
               started_at: str, completed_at: str | None = None,
               result_summary: str | None = None,
               error_message: str | None = None) -> None:
    """Record a schedule run (called from the server endpoint that Pi posts to)."""
    from datetime import datetime, timezone

    _ensure_schema()
    now = datetime.now(timezone.utc).isoformat()
    conn = sqlite3.connect(str(DB_PATH), check_same_thread=False)
    try:
        # Upsert the schedule so display queries have a name to show
        conn.execute(
            """INSERT INTO schedules (id, name, user_id, prompt, cron_expression,
                                      created_at, updated_at)
               VALUES (?, ?, ?, '', '', ?, ?)
               ON CONFLICT(id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at""",
            (schedule_id, schedule_name, user_id, now, now),
        )
        conn.execute(
            """INSERT INTO schedule_runs (schedule_id, user_id, started_at,
                                          completed_at, status, result_summary, error_message)
               VALUES (?, ?, ?, ?, ?, ?, ?)""",
            (schedule_id, user_id, started_at, completed_at, status,
             result_summary, error_message),
        )
        # Update schedule with last run info
        conn.execute(
            "UPDATE schedules SET run_count = COALESCE(run_count, 0) + 1, "
            "last_run = ?, last_result = ?, updated_at = ? WHERE id = ?",
            (completed_at or started_at, status, now, schedule_id),
        )
        conn.commit()
    finally:
        conn.close()


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


def _format_duration(start: datetime | None, end: datetime | None) -> str:
    if start is None or end is None:
        return "-"
    secs = int((end - start).total_seconds())
    if secs < 0:
        return "-"
    if secs < 60:
        return f"{secs}s"
    mins, rem = divmod(secs, 60)
    return f"{mins}m {rem}s"


def _truncate(text: str | None, limit: int) -> str:
    if not text:
        return ""
    text = text.strip()
    if len(text) <= limit:
        return text
    return text[: limit - 3] + "..."


def _missing_db_message() -> dict:
    return {
        "content": (
            "No scheduler database found at ~/.ludolph/schedules.db. "
            "Either no schedules have been created yet, or the Telegram bot "
            "hasn't been run on this machine."
        ),
        "error": None,
    }


def _list_schedule_runs(args: dict) -> dict:
    """Query recent schedule executions with optional filters."""
    conn = _connect()
    if conn is None:
        return _missing_db_message()

    name_filter = (args.get("schedule_name") or "").strip()
    status_filter = args.get("status")
    since_hours = args.get("since_hours")
    try:
        limit = int(args.get("limit") or _DEFAULT_RUN_LIMIT)
    except (TypeError, ValueError):
        limit = _DEFAULT_RUN_LIMIT
    limit = max(1, min(limit, _MAX_RUN_LIMIT))

    where_clauses = []
    params: list = []

    if name_filter:
        where_clauses.append("LOWER(s.name) LIKE ?")
        params.append(f"%{name_filter.lower()}%")

    if status_filter:
        where_clauses.append("r.status = ?")
        params.append(status_filter)

    if since_hours is not None:
        try:
            hours = int(since_hours)
        except (TypeError, ValueError):
            hours = 0
        if hours > 0:
            cutoff = (datetime.now(UTC) - timedelta(hours=hours)).isoformat()
            where_clauses.append("r.started_at >= ?")
            params.append(cutoff)

    where_sql = f"WHERE {' AND '.join(where_clauses)}" if where_clauses else ""
    sql = f"""
        SELECT r.id, r.schedule_id, r.started_at, r.completed_at, r.status,
               r.result_summary, r.error_message,
               COALESCE(s.name, '<deleted>') AS schedule_name
        FROM schedule_runs r
        LEFT JOIN schedules s ON s.id = r.schedule_id
        {where_sql}
        ORDER BY r.started_at DESC
        LIMIT ?
    """
    params.append(limit)

    try:
        rows = conn.execute(sql, params).fetchall()
    except sqlite3.Error as e:
        return {"content": "", "error": f"Failed to query scheduler DB: {e}"}
    finally:
        conn.close()

    if not rows:
        return {
            "content": "No schedule runs match the given filters.",
            "error": None,
        }

    lines = [f"Found {len(rows)} schedule run(s):\n"]
    for row in rows:
        started = _parse_iso(row["started_at"])
        completed = _parse_iso(row["completed_at"])
        age = _format_age(started)
        duration = _format_duration(started, completed)
        result_preview = _truncate(row["result_summary"], _RESULT_PREVIEW_CHARS)
        error_preview = _truncate(row["error_message"], _ERROR_PREVIEW_CHARS)

        lines.append(
            f"- [{row['status']}] {row['schedule_name']} — {age} (took {duration})"
        )
        if result_preview:
            lines.append(f"  result: {result_preview}")
        if error_preview:
            lines.append(f"  error: {error_preview}")

    return {"content": "\n".join(lines), "error": None}


def _schedule_health(args: dict) -> dict:  # noqa: ARG001
    """Proactive snapshot of scheduler health."""
    conn = _connect()
    if conn is None:
        return _missing_db_message()

    now = datetime.now(UTC)
    day_ago = (now - timedelta(hours=24)).isoformat()
    week_ago = (now - timedelta(days=7)).isoformat()

    try:
        totals = conn.execute(
            """
            SELECT
                COUNT(*) AS total_runs,
                SUM(CASE WHEN status = 'success' THEN 1 ELSE 0 END) AS ok,
                SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END) AS errors,
                SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END) AS running
            FROM schedule_runs
            """
        ).fetchone()

        failures_24h = conn.execute(
            """
            SELECT COALESCE(s.name, '<deleted>') AS name,
                   r.started_at, r.error_message, r.result_summary
            FROM schedule_runs r
            LEFT JOIN schedules s ON s.id = r.schedule_id
            WHERE r.status = 'error' AND r.started_at >= ?
            ORDER BY r.started_at DESC
            LIMIT 20
            """,
            (day_ago,),
        ).fetchall()

        failures_7d_count = conn.execute(
            """
            SELECT COALESCE(s.name, '<deleted>') AS name, COUNT(*) AS cnt
            FROM schedule_runs r
            LEFT JOIN schedules s ON s.id = r.schedule_id
            WHERE r.status = 'error' AND r.started_at >= ?
            GROUP BY r.schedule_id
            ORDER BY cnt DESC
            """,
            (week_ago,),
        ).fetchall()

        overdue = conn.execute(
            """
            SELECT name, next_run, last_run
            FROM schedules
            WHERE status = 'active'
              AND next_run IS NOT NULL
              AND next_run < ?
            ORDER BY next_run ASC
            LIMIT 20
            """,
            (now.isoformat(),),
        ).fetchall()

        active_count = conn.execute(
            "SELECT COUNT(*) AS c FROM schedules WHERE status = 'active'"
        ).fetchone()["c"]
    except sqlite3.Error as e:
        conn.close()
        return {"content": "", "error": f"Failed to query scheduler DB: {e}"}

    conn.close()

    lines = ["Schedule health snapshot:\n"]
    total = totals["total_runs"] or 0
    lines.append(
        f"Active schedules: {active_count} | "
        f"Total runs: {total} "
        f"(success: {totals['ok'] or 0}, "
        f"error: {totals['errors'] or 0}, "
        f"running: {totals['running'] or 0})"
    )

    lines.append("")
    if failures_24h:
        lines.append(f"Failures in last 24h ({len(failures_24h)}):")
        for row in failures_24h:
            age = _format_age(_parse_iso(row["started_at"]))
            err = _truncate(row["error_message"] or row["result_summary"], 160)
            lines.append(f"  - {row['name']} ({age}){': ' + err if err else ''}")
    else:
        lines.append("Failures in last 24h: none")

    lines.append("")
    if failures_7d_count:
        lines.append("Failure counts last 7d:")
        for row in failures_7d_count:
            lines.append(f"  - {row['name']}: {row['cnt']}")
    else:
        lines.append("Failure counts last 7d: none")

    lines.append("")
    if overdue:
        lines.append(f"Overdue active schedules ({len(overdue)}):")
        for row in overdue:
            next_age = _format_age(_parse_iso(row["next_run"]))
            last_age = _format_age(_parse_iso(row["last_run"])) if row["last_run"] else "never"
            lines.append(
                f"  - {row['name']}: due {next_age}, last ran {last_age}"
            )
    else:
        lines.append("Overdue active schedules: none")

    return {"content": "\n".join(lines), "error": None}


HANDLERS = {
    "list_schedule_runs": _list_schedule_runs,
    "schedule_health": _schedule_health,
}
