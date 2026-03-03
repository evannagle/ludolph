"""Unit tests for semantic search tools."""

import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).parent.parent.parent))

from mcp.security import init_security
from mcp.tools import call_tool
from mcp.tools.semantic import INDEX_PATH, _check_numpy


class TestSemanticSearchWithoutDependencies(unittest.TestCase):
    """Tests for semantic search when sentence-transformers is not installed."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")
        # Create test files
        (Path(self.tmpdir) / "test.md").write_text("# Test Note\nThis is a test.")

    def tearDown(self):
        shutil.rmtree(self.tmpdir)

    @patch("mcp.tools.semantic._get_model", return_value=None)
    def test_semantic_search_without_model(self, mock_model):
        """Should return helpful error when sentence-transformers not installed."""
        result = call_tool("semantic_search", {"query": "test"})
        self.assertIsNotNone(result["error"])
        self.assertIn("sentence-transformers", result["error"])

    @patch("mcp.tools.semantic._get_model", return_value=None)
    def test_rebuild_index_without_model(self, mock_model):
        """Should return helpful error when sentence-transformers not installed."""
        result = call_tool("rebuild_semantic_index", {})
        self.assertIsNotNone(result["error"])
        self.assertIn("sentence-transformers", result["error"])

    @patch("mcp.tools.semantic._get_model", return_value=None)
    def test_get_related_notes_without_model(self, mock_model):
        """Should return helpful error when sentence-transformers not installed."""
        result = call_tool("get_related_notes", {"path": "test.md"})
        self.assertIsNotNone(result["error"])
        self.assertIn("sentence-transformers", result["error"])


class TestSemanticSearchInputValidation(unittest.TestCase):
    """Tests for input validation in semantic search."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")

    def tearDown(self):
        shutil.rmtree(self.tmpdir)

    def test_semantic_search_empty_query(self):
        """Should require a query."""
        result = call_tool("semantic_search", {"query": ""})
        self.assertIsNotNone(result["error"])
        self.assertIn("required", result["error"].lower())

    def test_get_related_notes_empty_path(self):
        """Should require a path."""
        result = call_tool("get_related_notes", {"path": ""})
        self.assertIsNotNone(result["error"])
        self.assertIn("required", result["error"].lower())


@unittest.skipUnless(_check_numpy(), "numpy not installed")
class TestSemanticSearchWithMockedModel(unittest.TestCase):
    """Tests for semantic search with a mocked sentence transformer model."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")

        # Create test files
        (Path(self.tmpdir) / "apple.md").write_text("# Apples\nApples are delicious fruits.")
        (Path(self.tmpdir) / "banana.md").write_text("# Bananas\nBananas are yellow fruits.")
        (Path(self.tmpdir) / "notes").mkdir()
        (Path(self.tmpdir) / "notes" / "recipe.md").write_text(
            "# Apple Pie Recipe\nHow to make a pie."
        )

        # Create a mock model
        self.mock_model = MagicMock()
        import numpy as np

        # Make encode return normalized vectors for consistent similarity
        def mock_encode(text):
            # Return different vectors based on content keywords
            if "apple" in text.lower():
                return np.array([0.9, 0.1, 0.0])
            elif "banana" in text.lower():
                return np.array([0.1, 0.9, 0.0])
            elif "fruit" in text.lower():
                return np.array([0.5, 0.5, 0.0])
            elif "pie" in text.lower():
                return np.array([0.7, 0.1, 0.2])
            else:
                return np.array([0.3, 0.3, 0.4])

        self.mock_model.encode = mock_encode

    def tearDown(self):
        shutil.rmtree(self.tmpdir)
        # Clean up index file if created
        if INDEX_PATH.exists():
            INDEX_PATH.unlink()

    @patch("mcp.tools.semantic._get_model")
    def test_rebuild_index(self, mock_get_model):
        """Should build index from vault files."""
        mock_get_model.return_value = self.mock_model

        result = call_tool("rebuild_semantic_index", {})
        self.assertIsNone(result["error"])
        self.assertIn("Indexed 3 document", result["content"])
        self.assertTrue(INDEX_PATH.exists())

        # Verify index structure
        with open(INDEX_PATH) as f:
            index = json.load(f)
        self.assertEqual(len(index["documents"]), 3)
        self.assertTrue(all("embedding" in doc for doc in index["documents"]))
        self.assertTrue(all("path" in doc for doc in index["documents"]))
        self.assertTrue(all("title" in doc for doc in index["documents"]))

    @patch("mcp.tools.semantic._get_model")
    def test_semantic_search_without_index(self, mock_get_model):
        """Should error when index doesn't exist."""
        mock_get_model.return_value = self.mock_model

        # Make sure index doesn't exist
        if INDEX_PATH.exists():
            INDEX_PATH.unlink()

        result = call_tool("semantic_search", {"query": "apple"})
        self.assertIsNotNone(result["error"])
        self.assertIn("rebuild_semantic_index", result["error"])

    @patch("mcp.tools.semantic._get_model")
    def test_semantic_search_with_index(self, mock_get_model):
        """Should find semantically similar documents."""
        mock_get_model.return_value = self.mock_model

        # First build the index
        call_tool("rebuild_semantic_index", {})

        # Search for apple-related content
        result = call_tool("semantic_search", {"query": "apple"})
        self.assertIsNone(result["error"])
        self.assertIn("apple.md", result["content"])

    @patch("mcp.tools.semantic._get_model")
    def test_semantic_search_limit(self, mock_get_model):
        """Should respect limit parameter."""
        mock_get_model.return_value = self.mock_model

        # Build index
        call_tool("rebuild_semantic_index", {})

        # Search with limit 1
        result = call_tool("semantic_search", {"query": "fruit", "limit": 1})
        self.assertIsNone(result["error"])
        self.assertIn("1 result", result["content"])

    @patch("mcp.tools.semantic._get_model")
    def test_get_related_notes(self, mock_get_model):
        """Should find related notes."""
        mock_get_model.return_value = self.mock_model

        # Build index
        call_tool("rebuild_semantic_index", {})

        # Get notes related to apple.md
        result = call_tool("get_related_notes", {"path": "apple.md"})
        self.assertIsNone(result["error"])
        # Recipe should be related due to apple content
        self.assertIn("recipe.md", result["content"])

    @patch("mcp.tools.semantic._get_model")
    def test_get_related_notes_not_in_index(self, mock_get_model):
        """Should error for notes not in index."""
        mock_get_model.return_value = self.mock_model

        # Build index
        call_tool("rebuild_semantic_index", {})

        result = call_tool("get_related_notes", {"path": "nonexistent.md"})
        self.assertIsNotNone(result["error"])
        self.assertIn("not found", result["error"])


class TestSemanticIndexSkipsHiddenFiles(unittest.TestCase):
    """Tests that hidden files are properly skipped."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")

        # Create visible and hidden files
        (Path(self.tmpdir) / "visible.md").write_text("# Visible\nThis should be indexed.")
        (Path(self.tmpdir) / ".hidden.md").write_text("# Hidden\nThis should be skipped.")
        (Path(self.tmpdir) / ".obsidian").mkdir()
        (Path(self.tmpdir) / ".obsidian" / "config.md").write_text("# Config\nSkip this too.")

        # Mock model
        self.mock_model = MagicMock()
        import numpy as np

        self.mock_model.encode = lambda text: np.array([0.5, 0.5, 0.0])

    def tearDown(self):
        shutil.rmtree(self.tmpdir)
        if INDEX_PATH.exists():
            INDEX_PATH.unlink()

    @unittest.skipUnless(_check_numpy(), "numpy not installed")
    @patch("mcp.tools.semantic._get_model")
    def test_skips_hidden_files(self, mock_get_model):
        """Should skip files/directories starting with dot."""
        mock_get_model.return_value = self.mock_model

        result = call_tool("rebuild_semantic_index", {})
        self.assertIsNone(result["error"])
        self.assertIn("Indexed 1 document", result["content"])

        with open(INDEX_PATH) as f:
            index = json.load(f)
        paths = [doc["path"] for doc in index["documents"]]
        self.assertIn("visible.md", paths)
        self.assertNotIn(".hidden.md", paths)
        self.assertNotIn(".obsidian/config.md", paths)


if __name__ == "__main__":
    unittest.main()
