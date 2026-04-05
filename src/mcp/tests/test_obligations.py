"""Tests for the obligations MCP tool."""

import sys
import tempfile
import unittest
from datetime import UTC, datetime, timedelta
from pathlib import Path

_mcp_dir = Path(__file__).parent.parent
sys.path.insert(0, str(_mcp_dir))
sys.path.insert(0, str(_mcp_dir.parent))

from security import init_security  # noqa: E402
from tools import obligations as obligations_tool  # noqa: E402


def _task_md(
    *,
    task_id: str | None = None,
    title: str,
    client: str,
    status: str,
    priority: str = "Medium",
    due: str | None = None,
) -> str:
    lines = ["---"]
    if task_id is not None:
        lines.append(f"id: {task_id}")
    lines.append(f"title: {title}")
    lines.append(f"client: {client}")
    lines.append(f"status: {status}")
    lines.append(f"priority: {priority}")
    if due:
        lines.append(f"due: {due}")
    lines.append("type: Task")
    lines.append("---")
    lines.append("body")
    return "\n".join(lines)


class ObligationsTests(unittest.TestCase):
    """Exercise obligations tool's scanning, grouping, and rendering."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        self.vault = Path(self.tmpdir)
        init_security(self.vault, "test_token")
        (self.vault / "tasks").mkdir()

    def tearDown(self):
        import shutil

        shutil.rmtree(self.tmpdir)

    def _write_task(self, filename: str, content: str) -> Path:
        path = self.vault / "tasks" / filename
        path.write_text(content)
        return path

    def _write_hitlist(self, content: str) -> None:
        (self.vault / "Hitlist.md").write_text(content)

    def _write_recurring(self, content: str) -> None:
        target = self.vault / "+meta" / "contexts" / "Recurring.md"
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)

    # ------------------------------------------------------------------
    # _parse_date
    # ------------------------------------------------------------------

    def test_parse_date_accepts_iso(self):
        dt = obligations_tool._parse_date("2026-04-05")
        self.assertIsNotNone(dt)
        self.assertEqual((dt.year, dt.month, dt.day), (2026, 4, 5))

    def test_parse_date_strips_wikilinks(self):
        dt = obligations_tool._parse_date("[[2026-04-05]]")
        self.assertIsNotNone(dt)
        self.assertEqual(dt.month, 4)

    def test_parse_date_returns_none_for_garbage(self):
        self.assertIsNone(obligations_tool._parse_date("nope"))
        self.assertIsNone(obligations_tool._parse_date(None))
        self.assertIsNone(obligations_tool._parse_date(""))

    # ------------------------------------------------------------------
    # _scan_active_tasks
    # ------------------------------------------------------------------

    def test_scan_skips_done_tasks(self):
        self._write_task(
            "alive.md",
            _task_md(task_id="a-1", title="Alive", client="X", status="Started"),
        )
        self._write_task(
            "done.md",
            _task_md(task_id="d-1", title="Done", client="X", status="Done"),
        )
        tasks = obligations_tool._scan_active_tasks()
        ids = {t["id"] for t in tasks}
        self.assertEqual(ids, {"a-1"})

    def test_scan_recovers_id_from_filename_when_missing(self):
        self._write_task(
            "Personal - Thing (me-9999a).md",
            _task_md(
                title="Personal - Thing (me-9999a)",
                client="Personal",
                status="Started",
            ),
        )
        tasks = obligations_tool._scan_active_tasks()
        self.assertEqual(tasks[0]["id"], "me-9999a")

    def test_scan_parses_due_date(self):
        self._write_task(
            "t.md",
            _task_md(
                task_id="t-1",
                title="T",
                client="X",
                status="Started",
                due="2026-05-01",
            ),
        )
        tasks = obligations_tool._scan_active_tasks()
        self.assertIsNotNone(tasks[0]["due"])
        self.assertEqual(tasks[0]["due"].month, 5)

    def test_scan_returns_empty_when_no_tasks_dir(self):
        import shutil

        shutil.rmtree(self.vault / "tasks")
        self.assertEqual(obligations_tool._scan_active_tasks(), [])

    # ------------------------------------------------------------------
    # _group_by_client
    # ------------------------------------------------------------------

    def test_group_by_client_handles_missing_client(self):
        tasks = [
            {"id": "a", "client": "BeRad", "status": "Started"},
            {"id": "b", "client": None, "status": "Started"},
            {"id": "c", "client": "BeRad", "status": "Blocked"},
        ]
        groups = obligations_tool._group_by_client(tasks)
        self.assertEqual(len(groups["BeRad"]), 2)
        self.assertEqual(len(groups["Unassigned"]), 1)

    # ------------------------------------------------------------------
    # _read_hitlist_big_3
    # ------------------------------------------------------------------

    def test_read_big_3_returns_unchecked_items(self):
        self._write_hitlist(
            "## The Big 3\n\n"
            "- [ ] First thing\n"
            "- [x] Already done\n"
            "- [ ] [[Task Link|ts-123]] - Second thing\n"
            "\n## Small 3\n\n"
            "- [ ] Should not appear\n"
        )
        items = obligations_tool._read_hitlist_big_3(self.vault)
        self.assertEqual(len(items), 2)
        self.assertIn("First thing", items[0])

    def test_read_big_3_returns_empty_when_missing(self):
        self.assertEqual(obligations_tool._read_hitlist_big_3(self.vault), [])

    def test_read_big_3_returns_empty_when_section_missing(self):
        self._write_hitlist("## Other section\n\n- [ ] thing\n")
        self.assertEqual(obligations_tool._read_hitlist_big_3(self.vault), [])

    # ------------------------------------------------------------------
    # _strip_wikilinks
    # ------------------------------------------------------------------

    def test_strip_wikilinks_prefers_label(self):
        self.assertEqual(
            obligations_tool._strip_wikilinks("See [[target|Display Name]] now"),
            "See Display Name now",
        )

    def test_strip_wikilinks_uses_target_when_no_label(self):
        self.assertEqual(
            obligations_tool._strip_wikilinks("See [[target]] now"),
            "See target now",
        )

    # ------------------------------------------------------------------
    # _parse_recurring
    # ------------------------------------------------------------------

    def test_parse_recurring_computes_overdue(self):
        self._write_recurring(
            "## Monthly\n\n"
            "### Monthly Thing\n\n"
            "Last completed: 2026-01-01\n\n"
            "### Another Thing (nested note)\n\n"
            "Last completed: 2026-03-15\n\n"
            "## Weekly\n\n"
            "### Weekly Thing\n\n"
            "Last completed: 2026-03-20\n"
        )
        now = datetime(2026, 4, 5, tzinfo=UTC)
        items = obligations_tool._parse_recurring(self.vault, now)
        names = {i["name"]: i for i in items}
        self.assertIn("Monthly Thing", names)
        monthly = names["Monthly Thing"]
        self.assertEqual(monthly["cadence"], "Monthly")
        self.assertEqual(monthly["cadence_days"], 30)
        # Jan 1 → Apr 5 = 94 days, minus 30 = 64 overdue
        self.assertEqual(monthly["overdue_by"], 64)
        # Parenthetical stripped
        self.assertIn("Another Thing", names)

    def test_parse_recurring_skips_items_without_date(self):
        self._write_recurring("## Monthly\n\n### No Date Item\n\nSome body text without a date.\n")
        items = obligations_tool._parse_recurring(self.vault, datetime.now(UTC))
        self.assertEqual(items, [])

    def test_parse_recurring_returns_empty_when_missing(self):
        self.assertEqual(
            obligations_tool._parse_recurring(self.vault, datetime.now(UTC)),
            [],
        )

    # ------------------------------------------------------------------
    # _format_deadlines
    # ------------------------------------------------------------------

    def test_format_deadlines_surfaces_upcoming_and_overdue(self):
        now = datetime(2026, 4, 5, tzinfo=UTC)
        tasks = [
            {
                "id": "t-1",
                "title": "Overdue thing",
                "client": "X",
                "due": now - timedelta(days=3),
            },
            {
                "id": "t-2",
                "title": "Due soon (t-2)",
                "client": "X",
                "due": now + timedelta(days=5),
            },
            {
                "id": "t-3",
                "title": "Far future",
                "client": "X",
                "due": now + timedelta(days=90),
            },
            {
                "id": "t-4",
                "title": "No date",
                "client": "X",
                "due": None,
            },
        ]
        lines = obligations_tool._format_deadlines(tasks, now)
        text = "\n".join(lines)
        self.assertIn("t-1", text)
        self.assertIn("3d overdue", text)
        self.assertIn("t-2", text)
        self.assertIn("due in 5d", text)
        self.assertNotIn("t-3", text)
        # Title suffix stripped
        self.assertIn("Due soon", text)
        self.assertNotIn("Due soon (t-2)", text)

    def test_format_deadlines_empty_when_no_due_dates(self):
        now = datetime(2026, 4, 5, tzinfo=UTC)
        self.assertEqual(obligations_tool._format_deadlines([], now), [])

    # ------------------------------------------------------------------
    # _format_recurring
    # ------------------------------------------------------------------

    def test_format_recurring_shows_only_overdue_or_due(self):
        items = [
            {
                "name": "Overdue",
                "cadence": "Monthly",
                "cadence_days": 30,
                "last_completed": datetime(2026, 1, 1, tzinfo=UTC),
                "days_since": 94,
                "overdue_by": 64,
            },
            {
                "name": "Fresh",
                "cadence": "Monthly",
                "cadence_days": 30,
                "last_completed": datetime(2026, 4, 1, tzinfo=UTC),
                "days_since": 4,
                "overdue_by": -26,
            },
            {
                "name": "Unknown cadence",
                "cadence": "Zebra",
                "cadence_days": None,
                "last_completed": datetime(2026, 3, 1, tzinfo=UTC),
                "days_since": 35,
                "overdue_by": None,
            },
        ]
        lines = obligations_tool._format_recurring(items)
        text = "\n".join(lines)
        self.assertIn("Overdue", text)
        self.assertIn("overdue by 64d", text)
        self.assertNotIn("Fresh", text)
        self.assertNotIn("Unknown cadence", text)

    # ------------------------------------------------------------------
    # _obligations end-to-end
    # ------------------------------------------------------------------

    def test_obligations_end_to_end(self):
        self._write_task(
            "one.md",
            _task_md(task_id="br-1", title="Form fix", client="BeRad", status="Blocked"),
        )
        self._write_task(
            "two.md",
            _task_md(
                task_id="ts-1",
                title="Groups",
                client="Threadsourced",
                status="Started",
                priority="High",
            ),
        )
        self._write_hitlist("## The Big 3\n\n- [ ] Fix it\n\n## Small 3\n")
        self._write_recurring("## Monthly\n\n### Invoice\n\nLast completed: 2026-01-01\n")

        result = obligations_tool._obligations({})
        content = result["content"]
        self.assertIsNone(result["error"])
        self.assertIn("Clients with active work", content)
        self.assertIn("BeRad", content)
        self.assertIn("Threadsourced", content)
        self.assertIn("Hitlist (Big 3)", content)
        self.assertIn("Fix it", content)
        self.assertIn("Recurring", content)
        self.assertIn("Invoice", content)

    def test_obligations_empty_vault(self):
        result = obligations_tool._obligations({})
        self.assertIsNone(result["error"])
        self.assertIn("No active obligations", result["content"])


if __name__ == "__main__":
    unittest.main()
