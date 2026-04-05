"""Tests for index freshness exposure in vault_map and search_index."""

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


class IndexFreshnessTests(unittest.TestCase):
    """Exercise the freshness helpers and the tools that expose them."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        self.vault = Path(self.tmpdir)
        init_security(self.vault, "test_token")

        self.index_dir = self.vault / ".ludolph" / "index"
        self.index_dir.mkdir(parents=True)
        self.chunks_dir = self.index_dir / "chunks"
        self.chunks_dir.mkdir()

    def tearDown(self):
        import shutil

        shutil.rmtree(self.tmpdir)

    def _write_manifest(self, last_indexed_iso: str, file_count: int = 0) -> None:
        manifest = {
            "vault_path": str(self.vault),
            "tier": "standard",
            "file_count": file_count,
            "chunk_count": file_count,
            "last_indexed": last_indexed_iso,
            "version": 1,
            "folders": {},
        }
        (self.index_dir / "manifest.json").write_text(json.dumps(manifest))

    def _write_md(self, relative: str, content: str, mtime: float | None = None) -> Path:
        path = self.vault / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        if mtime is not None:
            os.utime(path, (mtime, mtime))
        return path

    # ------------------------------------------------------------------
    # _format_age
    # ------------------------------------------------------------------

    def test_format_age_handles_seconds_minutes_hours_days(self):
        now = datetime.now(UTC)
        self.assertEqual(
            index_tool._format_age((now - timedelta(seconds=10)).isoformat()).endswith("s ago"),
            True,
        )
        self.assertTrue(
            index_tool._format_age((now - timedelta(minutes=5)).isoformat()).endswith("m ago")
        )
        self.assertTrue(
            index_tool._format_age((now - timedelta(hours=3)).isoformat()).endswith("h ago")
        )
        self.assertTrue(
            index_tool._format_age((now - timedelta(days=2)).isoformat()).endswith("d ago")
        )

    def test_format_age_unknown_for_garbage(self):
        self.assertEqual(index_tool._format_age(""), "unknown")
        self.assertEqual(index_tool._format_age("not-a-date"), "unknown")

    # ------------------------------------------------------------------
    # _compute_freshness
    # ------------------------------------------------------------------

    def test_compute_freshness_counts_modified_files(self):
        # Indexed 1 hour ago; 2 files in vault, 1 touched after index.
        indexed_at = datetime.now(UTC) - timedelta(hours=1)
        old_mtime = (indexed_at - timedelta(minutes=10)).timestamp()
        new_mtime = (indexed_at + timedelta(minutes=30)).timestamp()
        self._write_md("old.md", "# old", mtime=old_mtime)
        self._write_md("new.md", "# new", mtime=new_mtime)
        self._write_manifest(indexed_at.isoformat(), file_count=2)

        manifest = json.loads((self.index_dir / "manifest.json").read_text())
        result = index_tool._compute_freshness(manifest)

        self.assertEqual(result["stale_file_count"], 1)
        self.assertEqual(result["total_vault_files"], 2)
        self.assertEqual(result["total_indexed_files"], 2)
        self.assertTrue(result["last_indexed_age"].endswith("ago"))
        self.assertFalse(result["scan_truncated"])

    def test_compute_freshness_skips_hidden_dirs(self):
        indexed_at = datetime.now(UTC)
        self._write_md(".obsidian/workspace.md", "# x")
        self._write_md("notes/real.md", "# real")
        self._write_manifest(indexed_at.isoformat(), file_count=1)

        manifest = json.loads((self.index_dir / "manifest.json").read_text())
        result = index_tool._compute_freshness(manifest)
        self.assertEqual(result["total_vault_files"], 1)

    def test_compute_freshness_handles_bad_timestamp(self):
        self._write_md("notes/one.md", "# one")
        self._write_manifest("garbage", file_count=1)

        manifest = json.loads((self.index_dir / "manifest.json").read_text())
        result = index_tool._compute_freshness(manifest)
        self.assertIsNone(result["stale_file_count"])
        self.assertIsNone(result["total_vault_files"])

    # ------------------------------------------------------------------
    # _vault_map tool handler
    # ------------------------------------------------------------------

    def test_vault_map_includes_freshness_lines(self):
        indexed_at = datetime.now(UTC) - timedelta(hours=2)
        self._write_md("a.md", "# a", mtime=(indexed_at + timedelta(minutes=1)).timestamp())
        self._write_md("b.md", "# b", mtime=(indexed_at - timedelta(hours=5)).timestamp())
        self._write_manifest(indexed_at.isoformat(), file_count=2)

        result = index_tool._vault_map({})
        content = result["content"]
        self.assertIsNone(result["error"])
        self.assertIn("Last indexed:", content)
        self.assertIn("ago", content)
        self.assertIn("Index freshness:", content)
        self.assertIn("1 of 2 vault file(s) modified", content)

    def test_vault_map_reports_missing_manifest(self):
        # No manifest written.
        result = index_tool._vault_map({})
        self.assertIn("No vault index found", result["content"])

    # ------------------------------------------------------------------
    # _search_index tool handler
    # ------------------------------------------------------------------

    def test_search_index_prepends_freshness_header(self):
        indexed_at = datetime.now(UTC) - timedelta(minutes=5)
        self._write_manifest(indexed_at.isoformat(), file_count=1)

        chunk_file = {
            "source": "notes/hello.md",
            "source_hash": "abc",
            "indexed_at": indexed_at.isoformat(),
            "tier": "standard",
            "frontmatter": {},
            "chunks": [
                {
                    "id": "c1",
                    "heading_path": ["Hello"],
                    "content": "the quick brown fox jumped",
                    "char_count": 26,
                    "position": 0,
                }
            ],
        }
        (self.chunks_dir / "notes").mkdir()
        (self.chunks_dir / "notes" / "hello.json").write_text(json.dumps(chunk_file))

        result = index_tool._search_index({"query": "fox"})
        self.assertIsNone(result["error"])
        self.assertIn("Index last updated:", result["content"])
        self.assertIn("ago", result["content"])
        self.assertIn("Found 1 matches", result["content"])

    def test_search_index_header_on_no_matches(self):
        indexed_at = datetime.now(UTC)
        self._write_manifest(indexed_at.isoformat())
        # Need a chunks dir to reach the matching path.
        (self.chunks_dir / "empty.json").write_text(
            json.dumps({"source": "empty.md", "chunks": []})
        )

        result = index_tool._search_index({"query": "nothing-here-xyz"})
        self.assertIn("Index last updated:", result["content"])
        self.assertIn("No matches found", result["content"])


if __name__ == "__main__":
    unittest.main()
