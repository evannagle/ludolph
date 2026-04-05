"""Tests for live_context bucketing surfaced via vault_map."""

import json
import os
import sys
import tempfile
import unittest
from datetime import UTC, datetime, timedelta
from pathlib import Path

_mcp_dir = Path(__file__).parent.parent
sys.path.insert(0, str(_mcp_dir))
sys.path.insert(0, str(_mcp_dir.parent))

from security import init_security  # noqa: E402
from tools import index as index_tool  # noqa: E402


def _task_md(
    *,
    task_id: str,
    title: str,
    client: str,
    status: str,
    priority: str,
    entries: list[str] | None = None,
) -> str:
    """Build a markdown task file with the frontmatter fields we care about."""
    lines = ["---"]
    lines.append(f"id: {task_id}")
    lines.append(f"title: {title}")
    lines.append(f"client: {client}")
    lines.append(f"status: {status}")
    lines.append(f"priority: {priority}")
    if entries:
        lines.append(f"entries: [{', '.join(entries)}]")
    lines.append("type: Task")
    lines.append("---")
    lines.append("")
    lines.append("body")
    return "\n".join(lines)


class LiveContextTests(unittest.TestCase):
    """Verify task scanning, bucketing, and vault_map rendering."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        self.vault = Path(self.tmpdir)
        init_security(self.vault, "test_token")
        self.tasks_dir = self.vault / "tasks"
        self.tasks_dir.mkdir()

    def tearDown(self):
        import shutil

        shutil.rmtree(self.tmpdir)

    def _write_task(self, filename: str, content: str, days_ago: float = 0) -> Path:
        path = self.tasks_dir / filename
        path.write_text(content)
        if days_ago:
            mtime = (datetime.now(UTC) - timedelta(days=days_ago)).timestamp()
            os.utime(path, (mtime, mtime))
        return path

    def _write_manifest(self) -> None:
        index_dir = self.vault / ".ludolph" / "index"
        index_dir.mkdir(parents=True)
        (index_dir / "manifest.json").write_text(
            json.dumps(
                {
                    "vault_path": str(self.vault),
                    "tier": "standard",
                    "file_count": 0,
                    "chunk_count": 0,
                    "last_indexed": datetime.now(UTC).isoformat(),
                    "version": 1,
                    "folders": {},
                }
            )
        )

    # ------------------------------------------------------------------
    # _latest_entry_date
    # ------------------------------------------------------------------

    def test_latest_entry_date_picks_newest_wikilink(self):
        latest = index_tool._latest_entry_date(["[[2026-01-05]]", "[[2026-04-04]]"])
        self.assertIsNotNone(latest)
        self.assertEqual(latest.year, 2026)
        self.assertEqual(latest.month, 4)
        self.assertEqual(latest.day, 4)

    def test_latest_entry_date_accepts_string(self):
        latest = index_tool._latest_entry_date("[[2026-02-14]]")
        self.assertIsNotNone(latest)
        self.assertEqual(latest.month, 2)

    def test_latest_entry_date_ignores_garbage(self):
        self.assertIsNone(index_tool._latest_entry_date(None))
        self.assertIsNone(index_tool._latest_entry_date("no dates here"))
        self.assertIsNone(index_tool._latest_entry_date([]))

    # ------------------------------------------------------------------
    # _scan_tasks
    # ------------------------------------------------------------------

    def test_scan_tasks_reads_frontmatter(self):
        self._write_task(
            "one.md",
            _task_md(
                task_id="t-1",
                title="Task One",
                client="Personal",
                status="Started",
                priority="High",
            ),
        )
        tasks = index_tool._scan_tasks()
        self.assertEqual(len(tasks), 1)
        self.assertEqual(tasks[0]["id"], "t-1")
        self.assertEqual(tasks[0]["status"], "Started")
        self.assertEqual(tasks[0]["priority"], "High")
        self.assertEqual(tasks[0]["client"], "Personal")

    def test_scan_tasks_skips_done_subdir(self):
        done = self.tasks_dir / "done"
        done.mkdir()
        (done / "finished.md").write_text(
            _task_md(
                task_id="t-done",
                title="Finished",
                client="X",
                status="Done",
                priority="Low",
            )
        )
        self._write_task(
            "active.md",
            _task_md(
                task_id="t-active",
                title="Active",
                client="X",
                status="Started",
                priority="Medium",
            ),
        )
        tasks = index_tool._scan_tasks()
        ids = {t["id"] for t in tasks}
        self.assertEqual(ids, {"t-active"})

    def test_scan_tasks_returns_empty_when_no_tasks_dir(self):
        import shutil

        shutil.rmtree(self.tasks_dir)
        self.assertEqual(index_tool._scan_tasks(), [])

    def test_scan_tasks_skips_files_without_frontmatter(self):
        (self.tasks_dir / "bare.md").write_text("no frontmatter here\n")
        self._write_task(
            "good.md",
            _task_md(
                task_id="t-good",
                title="Good",
                client="X",
                status="Pending",
                priority="Low",
            ),
        )
        tasks = index_tool._scan_tasks()
        self.assertEqual(len(tasks), 1)
        self.assertEqual(tasks[0]["id"], "t-good")

    # ------------------------------------------------------------------
    # _bucket_tasks
    # ------------------------------------------------------------------

    def _make_task(
        self,
        *,
        task_id: str = "t-x",
        title: str = "X",
        client: str = "C",
        status: str = "Started",
        priority: str = "Medium",
        days_ago: float = 0,
    ) -> dict:
        mtime = (datetime.now(UTC) - timedelta(days=days_ago)).timestamp()
        return {
            "id": task_id,
            "title": title,
            "client": client,
            "status": status,
            "priority": priority,
            "path": f"tasks/{task_id}.md",
            "mtime": mtime,
            "entries_latest": None,
        }

    def test_bucket_active_includes_recent(self):
        tasks = [
            self._make_task(task_id="fresh", days_ago=0.5),
            self._make_task(task_id="old", days_ago=30),
        ]
        buckets = index_tool._bucket_tasks(tasks)
        active_ids = {t["id"] for t in buckets["active_today"]}
        self.assertIn("fresh", active_ids)
        self.assertNotIn("old", active_ids)

    def test_bucket_stalled_requires_started_and_idle(self):
        tasks = [
            self._make_task(task_id="started-idle", status="Started", days_ago=20),
            self._make_task(task_id="started-fresh", status="Started", days_ago=1),
            self._make_task(task_id="pending-idle", status="Pending", days_ago=20),
        ]
        buckets = index_tool._bucket_tasks(tasks)
        stalled_ids = {t["id"] for t in buckets["stalled"]}
        self.assertEqual(stalled_ids, {"started-idle"})

    def test_bucket_blocked_collects_blocked(self):
        tasks = [
            self._make_task(task_id="b1", status="Blocked", days_ago=5),
            self._make_task(task_id="s1", status="Started", days_ago=5),
        ]
        buckets = index_tool._bucket_tasks(tasks)
        blocked_ids = {t["id"] for t in buckets["blocked"]}
        self.assertEqual(blocked_ids, {"b1"})

    def test_bucket_urgent_from_priority(self):
        tasks = [
            self._make_task(task_id="u1", priority="Urgent", days_ago=5),
            self._make_task(task_id="h1", priority="High", days_ago=5),
            self._make_task(task_id="m1", priority="Medium", days_ago=5),
            self._make_task(task_id="done", priority="High", status="Done", days_ago=5),
        ]
        buckets = index_tool._bucket_tasks(tasks)
        urgent_ids = {t["id"] for t in buckets["urgent"]}
        self.assertEqual(urgent_ids, {"u1", "h1"})

    def test_bucket_excludes_done(self):
        tasks = [
            self._make_task(task_id="d1", status="Done", days_ago=0),
        ]
        buckets = index_tool._bucket_tasks(tasks)
        self.assertEqual(buckets["active_today"], [])
        self.assertEqual(buckets["urgent"], [])

    def test_bucket_strips_id_suffix_from_title(self):
        tasks = [
            self._make_task(
                task_id="me-0403a",
                title="Personal - Thing (me-0403a)",
                days_ago=0,
            ),
        ]
        buckets = index_tool._bucket_tasks(tasks)
        self.assertEqual(buckets["active_today"][0]["title"], "Personal - Thing")

    # ------------------------------------------------------------------
    # _vault_map integration
    # ------------------------------------------------------------------

    def test_vault_map_includes_live_context_section(self):
        self._write_manifest()
        self._write_task(
            "t1.md",
            _task_md(
                task_id="t-1",
                title="Urgent thing",
                client="Personal",
                status="Blocked",
                priority="High",
            ),
        )
        result = index_tool._vault_map({})
        self.assertIsNone(result["error"])
        self.assertIn("Live context", result["content"])
        self.assertIn("Blocked", result["content"])
        self.assertIn("t-1", result["content"])

    def test_vault_map_omits_live_context_when_no_tasks(self):
        self._write_manifest()
        result = index_tool._vault_map({})
        self.assertIsNone(result["error"])
        self.assertNotIn("Live context", result["content"])

    def test_format_live_context_returns_empty_for_empty_buckets(self):
        empty = {
            "active_today": [],
            "stalled": [],
            "blocked": [],
            "urgent": [],
        }
        self.assertEqual(index_tool._format_live_context(empty), [])


if __name__ == "__main__":
    unittest.main()
