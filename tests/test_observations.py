"""Tests for the observations store."""

import sys
import tempfile
import unittest
from pathlib import Path

# Add parent to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent / "src" / "mcp"))

from tools.observations import SqliteObservationStore


class TestSqliteObservationStore(unittest.TestCase):
    """Tests for SqliteObservationStore."""

    def setUp(self):
        """Create a temp store for each test."""
        self.tmpdir = tempfile.mkdtemp()
        self.db_path = Path(self.tmpdir) / "test_observations.db"
        self.store = SqliteObservationStore(self.db_path)
        self.user_id = 12345

    def test_save_and_retrieve(self):
        """Saved observations can be retrieved."""
        result = self.store.save(
            self.user_id, "Prefers Quickies for hitlist items", "preference", "Default hitlist"
        )

        self.assertIn("id", result)
        self.assertEqual(result["category"], "preference")

        obs = self.store.get([result["id"]])
        self.assertEqual(len(obs), 1)
        self.assertEqual(obs[0]["text"], "Prefers Quickies for hitlist items")
        self.assertEqual(obs[0]["title"], "Default hitlist")

    def test_search_finds_matching(self):
        """Search returns matching observations."""
        self.store.save(self.user_id, "Elvis is the user's son", "fact", "Son")
        self.store.save(self.user_id, "Book proposal due April 1", "context", "Deadline")

        results = self.store.search(self.user_id, "Elvis")
        self.assertEqual(len(results), 1)
        self.assertIn("Elvis", results[0]["text"])

    def test_search_filters_by_category(self):
        """Search can filter by category."""
        self.store.save(self.user_id, "Likes dark mode", "preference")
        self.store.save(self.user_id, "Works at Acme", "fact")

        results = self.store.search(self.user_id, "dark OR Acme", category="preference")
        self.assertEqual(len(results), 1)
        self.assertIn("dark mode", results[0]["text"])

    def test_delete_removes_observation(self):
        """Delete removes the observation."""
        result = self.store.save(self.user_id, "Temporary note", "context")
        obs_id = result["id"]

        self.assertTrue(self.store.delete(obs_id, self.user_id))
        self.assertEqual(len(self.store.get([obs_id])), 0)

    def test_delete_wrong_user_fails(self):
        """Cannot delete another user's observation."""
        result = self.store.save(self.user_id, "My note", "fact")
        self.assertFalse(self.store.delete(result["id"], 99999))

    def test_recent_returns_newest_first(self):
        """Recent observations are ordered newest first."""
        self.store.save(self.user_id, "First", "fact")
        self.store.save(self.user_id, "Second", "fact")
        self.store.save(self.user_id, "Third", "fact")

        recent = self.store.recent(self.user_id, limit=2)
        self.assertEqual(len(recent), 2)
        self.assertEqual(recent[0]["text"], "Third")
        self.assertEqual(recent[1]["text"], "Second")

    def test_users_are_isolated(self):
        """Different users have separate observations."""
        self.store.save(100, "User A fact", "fact")
        self.store.save(200, "User B fact", "fact")

        self.assertEqual(len(self.store.recent(100)), 1)
        self.assertEqual(len(self.store.recent(200)), 1)
        self.assertEqual(self.store.recent(100)[0]["text"], "User A fact")

    def test_list_all_with_category_filter(self):
        """list_all can filter by category."""
        self.store.save(self.user_id, "Pref 1", "preference")
        self.store.save(self.user_id, "Fact 1", "fact")
        self.store.save(self.user_id, "Pref 2", "preference")

        prefs = self.store.list_all(self.user_id, category="preference")
        self.assertEqual(len(prefs), 2)

        facts = self.store.list_all(self.user_id, category="fact")
        self.assertEqual(len(facts), 1)

    def test_search_empty_result(self):
        """Search returns empty list when nothing matches."""
        results = self.store.search(self.user_id, "nonexistent")
        self.assertEqual(results, [])


if __name__ == "__main__":
    unittest.main()
