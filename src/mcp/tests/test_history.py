"""Tests for the conversation history lookup tool."""

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

from tools import history as history_tool  # noqa: E402


def _init_db(path: Path) -> sqlite3.Connection:
    """Create the conversation messages schema (mirrors Rust memory.rs)."""
    conn = sqlite3.connect(str(path))
    conn.executescript(
        """
        CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            timestamp TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            persisted INTEGER DEFAULT 0
        );
        CREATE INDEX idx_user_time ON messages(user_id, timestamp DESC);
        """
    )
    conn.commit()
    return conn


class HasMentionedBeforeTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        self.db_path = Path(self.tmpdir) / "conversations.db"
        self.conn = _init_db(self.db_path)
        self._orig_db = history_tool.DB_PATH
        history_tool.DB_PATH = self.db_path

    def tearDown(self):
        self.conn.close()
        history_tool.DB_PATH = self._orig_db
        import shutil

        shutil.rmtree(self.tmpdir)

    def _insert(
        self,
        role: str,
        content: str,
        *,
        user_id: int = 1,
        when: datetime | None = None,
    ) -> None:
        ts = (when or datetime.now(UTC)).isoformat()
        self.conn.execute(
            "INSERT INTO messages (user_id, timestamp, role, content) VALUES (?, ?, ?, ?)",
            (user_id, ts, role, content),
        )
        self.conn.commit()

    # ------------------------------------------------------------------
    # basic matching
    # ------------------------------------------------------------------

    def test_finds_matching_assistant_message(self):
        self._insert("assistant", "The vault lives at ~/Vaults/Noggin")
        result = history_tool._has_mentioned_before({"topic": "vault"})
        self.assertIsNone(result["error"])
        self.assertIn("1 prior mention", result["content"])
        self.assertIn("~/Vaults/Noggin", result["content"])

    def test_no_matches_returns_friendly_message(self):
        self._insert("assistant", "Hello there")
        result = history_tool._has_mentioned_before({"topic": "quantum physics"})
        self.assertIn("No prior mentions", result["content"])
        self.assertIsNone(result["error"])

    def test_match_is_case_insensitive(self):
        self._insert("assistant", "Your Raspberry Pi is healthy")
        result = history_tool._has_mentioned_before({"topic": "RASPBERRY"})
        self.assertIn("1 prior mention", result["content"])

    def test_defaults_to_assistant_role_only(self):
        self._insert("user", "tell me about llamas")
        self._insert("assistant", "okay here is something else")
        result = history_tool._has_mentioned_before({"topic": "llamas"})
        # By default, don't match user messages
        self.assertIn("No prior mentions", result["content"])

    def test_include_user_messages_flag_works(self):
        self._insert("user", "remind me about llamas")
        self._insert("assistant", "sure thing")
        result = history_tool._has_mentioned_before(
            {"topic": "llamas", "include_user_messages": True}
        )
        self.assertIn("1 prior mention", result["content"])
        self.assertIn("[user]", result["content"])

    # ------------------------------------------------------------------
    # filters
    # ------------------------------------------------------------------

    def test_since_hours_filters_older_messages(self):
        old = datetime.now(UTC) - timedelta(hours=72)
        self._insert("assistant", "old talk of widgets", when=old)
        self._insert("assistant", "new talk of widgets")
        result = history_tool._has_mentioned_before(
            {"topic": "widgets", "since_hours": 24}
        )
        self.assertIn("1 prior mention", result["content"])
        self.assertIn("new talk", result["content"])
        self.assertNotIn("old talk", result["content"])

    def test_limit_is_clamped(self):
        for i in range(20):
            self._insert("assistant", f"widgets run {i}")
        result = history_tool._has_mentioned_before({"topic": "widgets", "limit": 3})
        # Should cap at 3 results shown, but count reflects reality
        self.assertIn("20 prior mention", result["content"])
        # Only 3 entries should appear in the preview list
        entry_lines = [
            ln for ln in result["content"].splitlines() if ln.startswith("- [")
        ]
        self.assertEqual(len(entry_lines), 3)

    def test_repetition_hint_surfaces_at_three_plus(self):
        for _ in range(4):
            self._insert("assistant", "the answer is 42")
        result = history_tool._has_mentioned_before({"topic": "42"})
        self.assertIn("4 prior mention", result["content"])
        self.assertIn("mentioned this", result["content"].lower())

    def test_no_repetition_hint_below_three(self):
        for _ in range(2):
            self._insert("assistant", "the answer is 42")
        result = history_tool._has_mentioned_before({"topic": "42"})
        self.assertNotIn("mentioned this", result["content"].lower())

    # ------------------------------------------------------------------
    # edge cases
    # ------------------------------------------------------------------

    def test_missing_db_returns_friendly_message(self):
        history_tool.DB_PATH = Path(self.tmpdir) / "nope.db"
        result = history_tool._has_mentioned_before({"topic": "anything"})
        self.assertIn("No conversation history", result["content"])
        self.assertIsNone(result["error"])

    def test_empty_topic_returns_error(self):
        result = history_tool._has_mentioned_before({"topic": "  "})
        self.assertIsNotNone(result["error"])

    def test_missing_topic_returns_error(self):
        result = history_tool._has_mentioned_before({})
        self.assertIsNotNone(result["error"])

    def test_unicode_topic_works(self):
        self._insert("assistant", "café is a great word")
        result = history_tool._has_mentioned_before({"topic": "café"})
        self.assertIn("1 prior mention", result["content"])

    def test_sql_wildcards_are_escaped(self):
        # Messages that would match if % or _ were treated as wildcards
        self._insert("assistant", "percent sign p%rcent")
        self._insert("assistant", "regular text without wildcard")
        result = history_tool._has_mentioned_before({"topic": "p%r"})
        # Should literally match "p%r"
        self.assertIn("1 prior mention", result["content"])
        self.assertIn("p%rcent", result["content"])

    def test_most_recent_first(self):
        old = datetime.now(UTC) - timedelta(hours=5)
        newer = datetime.now(UTC) - timedelta(hours=1)
        self._insert("assistant", "widgets first mention", when=old)
        self._insert("assistant", "widgets second mention", when=newer)
        result = history_tool._has_mentioned_before({"topic": "widgets"})
        # Most recent should appear before older one in output
        content = result["content"]
        self.assertLess(
            content.index("second mention"), content.index("first mention")
        )

    def test_content_preview_is_truncated(self):
        long_content = "widgets " + ("x" * 500)
        self._insert("assistant", long_content)
        result = history_tool._has_mentioned_before({"topic": "widgets"})
        self.assertIn("...", result["content"])
        # Should not include 400 consecutive x's
        self.assertNotIn("x" * 400, result["content"])


if __name__ == "__main__":
    unittest.main()
