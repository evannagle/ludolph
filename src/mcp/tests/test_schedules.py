"""Tests for the schedule execution logging tools."""

from __future__ import annotations

import sqlite3
import sys
import tempfile
import unittest
from datetime import UTC, datetime, timedelta
from pathlib import Path

_mcp_dir = Path(__file__).parent.parent
sys.path.insert(0, str(_mcp_dir))
sys.path.insert(0, str(_mcp_dir.parent))

from tools import schedules as sched_tool  # noqa: E402


def _init_db(path: Path) -> sqlite3.Connection:
    """Create the scheduler schema (mirrors Rust scheduler.rs)."""
    conn = sqlite3.connect(str(path))
    conn.executescript(
        """
        CREATE TABLE schedules (
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
        CREATE TABLE schedule_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            schedule_id TEXT NOT NULL,
            user_id INTEGER NOT NULL,
            started_at TEXT NOT NULL,
            completed_at TEXT,
            status TEXT NOT NULL,
            result_summary TEXT,
            error_message TEXT
        );
        """
    )
    conn.commit()
    return conn


class ScheduleToolsTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        self.db_path = Path(self.tmpdir) / "schedules.db"
        self.conn = _init_db(self.db_path)
        # Redirect the tool module to our temp DB
        self._orig_db = sched_tool.DB_PATH
        sched_tool.DB_PATH = self.db_path

    def tearDown(self):
        self.conn.close()
        sched_tool.DB_PATH = self._orig_db
        import shutil

        shutil.rmtree(self.tmpdir)

    def _insert_schedule(
        self,
        schedule_id: str,
        name: str,
        *,
        status: str = "active",
        next_run: datetime | None = None,
        last_run: datetime | None = None,
    ) -> None:
        now = datetime.now(UTC).isoformat()
        self.conn.execute(
            """INSERT INTO schedules
               (id, user_id, name, prompt, cron_expression, status,
                created_at, updated_at, next_run, last_run)
               VALUES (?, 1, ?, 'p', '0 9 * * *', ?, ?, ?, ?, ?)""",
            (
                schedule_id,
                name,
                status,
                now,
                now,
                next_run.isoformat() if next_run else None,
                last_run.isoformat() if last_run else None,
            ),
        )
        self.conn.commit()

    def _insert_run(
        self,
        schedule_id: str,
        status: str,
        *,
        started_at: datetime | None = None,
        completed_at: datetime | None = None,
        result: str | None = None,
        error: str | None = None,
    ) -> None:
        started_at = started_at or datetime.now(UTC)
        self.conn.execute(
            """INSERT INTO schedule_runs
               (schedule_id, user_id, started_at, completed_at, status,
                result_summary, error_message)
               VALUES (?, 1, ?, ?, ?, ?, ?)""",
            (
                schedule_id,
                started_at.isoformat(),
                completed_at.isoformat() if completed_at else None,
                status,
                result,
                error,
            ),
        )
        self.conn.commit()

    # ------------------------------------------------------------------
    # list_schedule_runs
    # ------------------------------------------------------------------

    def test_list_runs_returns_recent_entries(self):
        self._insert_schedule("s1", "Daily Digest")
        start = datetime.now(UTC) - timedelta(minutes=5)
        end = start + timedelta(seconds=12)
        self._insert_run(
            "s1",
            "success",
            started_at=start,
            completed_at=end,
            result="Processed 42 items",
        )

        result = sched_tool._list_schedule_runs({})
        self.assertIsNone(result["error"])
        self.assertIn("Daily Digest", result["content"])
        self.assertIn("[success]", result["content"])
        self.assertIn("Processed 42 items", result["content"])
        self.assertIn("12s", result["content"])

    def test_list_runs_filters_by_name(self):
        self._insert_schedule("a", "Alpha")
        self._insert_schedule("b", "Beta")
        self._insert_run("a", "success")
        self._insert_run("b", "success")

        result = sched_tool._list_schedule_runs({"schedule_name": "alpha"})
        self.assertIn("Alpha", result["content"])
        self.assertNotIn("Beta", result["content"])

    def test_list_runs_filters_by_status(self):
        self._insert_schedule("s1", "One")
        self._insert_run("s1", "success")
        self._insert_run("s1", "error", error="boom")

        result = sched_tool._list_schedule_runs({"status": "error"})
        self.assertIn("[error]", result["content"])
        self.assertIn("boom", result["content"])
        self.assertNotIn("[success]", result["content"])

    def test_list_runs_filters_by_since_hours(self):
        self._insert_schedule("s1", "One")
        old_start = datetime.now(UTC) - timedelta(hours=48)
        self._insert_run("s1", "success", started_at=old_start)
        self._insert_run("s1", "success")

        result = sched_tool._list_schedule_runs({"since_hours": 24})
        # Should only include the recent run (1 entry)
        self.assertIn("Found 1 schedule run", result["content"])

    def test_list_runs_empty(self):
        self._insert_schedule("s1", "One")
        result = sched_tool._list_schedule_runs({})
        self.assertIn("No schedule runs match", result["content"])

    def test_list_runs_handles_deleted_schedule(self):
        # Run references a schedule_id that doesn't exist in schedules table
        self._insert_run("ghost", "success")
        result = sched_tool._list_schedule_runs({})
        self.assertIn("<deleted>", result["content"])

    def test_list_runs_truncates_long_output(self):
        self._insert_schedule("s1", "One")
        self._insert_run("s1", "success", result="x" * 500)
        result = sched_tool._list_schedule_runs({})
        self.assertIn("...", result["content"])
        # Shouldn't contain the full 500-char string
        self.assertNotIn("x" * 400, result["content"])

    def test_list_runs_missing_db(self):
        sched_tool.DB_PATH = Path(self.tmpdir) / "nope.db"
        result = sched_tool._list_schedule_runs({})
        self.assertIn("No scheduler database", result["content"])

    def test_list_runs_limit_is_clamped(self):
        self._insert_schedule("s1", "One")
        for _ in range(5):
            self._insert_run("s1", "success")
        result = sched_tool._list_schedule_runs({"limit": 2})
        self.assertIn("Found 2 schedule run", result["content"])

    # ------------------------------------------------------------------
    # schedule_health
    # ------------------------------------------------------------------

    def test_schedule_health_reports_no_failures_cleanly(self):
        self._insert_schedule("s1", "Healthy")
        self._insert_run("s1", "success")
        result = sched_tool._schedule_health({})
        self.assertIsNone(result["error"])
        self.assertIn("Failures in last 24h: none", result["content"])
        self.assertIn("Overdue active schedules: none", result["content"])

    def test_schedule_health_lists_recent_failures(self):
        self._insert_schedule("s1", "Breaking Task")
        self._insert_run(
            "s1",
            "error",
            started_at=datetime.now(UTC) - timedelta(hours=2),
            error="network timeout",
        )
        result = sched_tool._schedule_health({})
        self.assertIn("Breaking Task", result["content"])
        self.assertIn("network timeout", result["content"])
        self.assertIn("Failures in last 24h (1)", result["content"])

    def test_schedule_health_counts_weekly_failures(self):
        self._insert_schedule("s1", "Flaky")
        for _ in range(3):
            self._insert_run(
                "s1",
                "error",
                started_at=datetime.now(UTC) - timedelta(days=2),
                error="x",
            )
        result = sched_tool._schedule_health({})
        self.assertIn("Flaky: 3", result["content"])

    def test_schedule_health_flags_overdue_schedules(self):
        self._insert_schedule(
            "s1",
            "Late Riser",
            next_run=datetime.now(UTC) - timedelta(hours=3),
        )
        result = sched_tool._schedule_health({})
        self.assertIn("Overdue active schedules (1)", result["content"])
        self.assertIn("Late Riser", result["content"])

    def test_schedule_health_missing_db(self):
        sched_tool.DB_PATH = Path(self.tmpdir) / "nope.db"
        result = sched_tool._schedule_health({})
        self.assertIn("No scheduler database", result["content"])


if __name__ == "__main__":
    unittest.main()
