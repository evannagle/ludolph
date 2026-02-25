"""Unit tests for MCP tools."""

import os
import tempfile
import unittest
from pathlib import Path

# Add parent to path for imports
import sys
sys.path.insert(0, str(Path(__file__).parent.parent.parent))

from mcp.security import init_security, safe_path
from mcp.tools import call_tool


class TestSafePath(unittest.TestCase):
    """Tests for path validation."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")
        # Create test structure
        (Path(self.tmpdir) / "notes").mkdir()
        (Path(self.tmpdir) / "notes" / "test.md").write_text("test content")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_safe_path_accepts_valid_paths(self):
        """Valid paths within vault should resolve."""
        result = safe_path("notes/test.md")
        self.assertIsNotNone(result)
        self.assertTrue(result.exists())

    def test_safe_path_rejects_traversal(self):
        """Paths with .. should be rejected."""
        self.assertIsNone(safe_path("../etc/passwd"))
        self.assertIsNone(safe_path("notes/../../../etc/passwd"))
        self.assertIsNone(safe_path(".."))

    def test_safe_path_handles_empty(self):
        """Empty path returns vault root."""
        result = safe_path("")
        self.assertIsNotNone(result)
        # Use resolve() to handle /var -> /private/var symlink on macOS
        self.assertEqual(result, Path(self.tmpdir).resolve())

    def test_safe_path_handles_dot(self):
        """Single dot returns vault root."""
        result = safe_path(".")
        self.assertIsNotNone(result)
        # Use resolve() to handle /var -> /private/var symlink on macOS
        self.assertEqual(result, Path(self.tmpdir).resolve())


class TestReadFile(unittest.TestCase):
    """Tests for read_file tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")
        (Path(self.tmpdir) / "test.txt").write_text("hello world")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_read_existing_file(self):
        """Should read existing file content."""
        result = call_tool("read_file", {"path": "test.txt"})
        self.assertIsNone(result["error"])
        self.assertEqual(result["content"], "hello world")

    def test_read_missing_file(self):
        """Should return error for missing file."""
        result = call_tool("read_file", {"path": "missing.txt"})
        self.assertIsNotNone(result["error"])
        self.assertIn("not found", result["error"])

    def test_read_invalid_path(self):
        """Should reject path traversal."""
        result = call_tool("read_file", {"path": "../etc/passwd"})
        self.assertIsNotNone(result["error"])
        self.assertIn("Invalid", result["error"])


class TestWriteFile(unittest.TestCase):
    """Tests for write_file tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_write_new_file(self):
        """Should create new file."""
        result = call_tool("write_file", {"path": "new.txt", "content": "hello"})
        self.assertIsNone(result["error"])
        self.assertTrue((Path(self.tmpdir) / "new.txt").exists())
        self.assertEqual((Path(self.tmpdir) / "new.txt").read_text(), "hello")

    def test_write_creates_directories(self):
        """Should create parent directories."""
        result = call_tool("write_file", {"path": "a/b/c/file.txt", "content": "deep"})
        self.assertIsNone(result["error"])
        self.assertTrue((Path(self.tmpdir) / "a" / "b" / "c" / "file.txt").exists())

    def test_write_overwrites_existing(self):
        """Should overwrite existing file."""
        (Path(self.tmpdir) / "existing.txt").write_text("old")
        result = call_tool("write_file", {"path": "existing.txt", "content": "new"})
        self.assertIsNone(result["error"])
        self.assertEqual((Path(self.tmpdir) / "existing.txt").read_text(), "new")


class TestAppendFile(unittest.TestCase):
    """Tests for append_file tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_append_to_existing(self):
        """Should append to existing file."""
        (Path(self.tmpdir) / "test.txt").write_text("hello")
        result = call_tool("append_file", {"path": "test.txt", "content": " world"})
        self.assertIsNone(result["error"])
        self.assertEqual((Path(self.tmpdir) / "test.txt").read_text(), "hello\n world")

    def test_append_creates_file(self):
        """Should create file if missing."""
        result = call_tool("append_file", {"path": "new.txt", "content": "hello"})
        self.assertIsNone(result["error"])
        self.assertEqual((Path(self.tmpdir) / "new.txt").read_text(), "hello")

    def test_append_newline_handling(self):
        """Should add newline before appended content if needed."""
        (Path(self.tmpdir) / "test.txt").write_text("line1\n")
        result = call_tool("append_file", {"path": "test.txt", "content": "line2"})
        self.assertIsNone(result["error"])
        # File already ends with newline, so no extra newline added
        self.assertEqual((Path(self.tmpdir) / "test.txt").read_text(), "line1\nline2")


class TestDeleteFile(unittest.TestCase):
    """Tests for delete_file tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")
        (Path(self.tmpdir) / "delete_me.txt").write_text("goodbye")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_delete_existing_file(self):
        """Should delete existing file."""
        result = call_tool("delete_file", {"path": "delete_me.txt"})
        self.assertIsNone(result["error"])
        self.assertFalse((Path(self.tmpdir) / "delete_me.txt").exists())

    def test_delete_missing_file(self):
        """Should return error for missing file."""
        result = call_tool("delete_file", {"path": "missing.txt"})
        self.assertIsNotNone(result["error"])
        self.assertIn("not found", result["error"])


class TestMoveFile(unittest.TestCase):
    """Tests for move_file tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")
        (Path(self.tmpdir) / "source.txt").write_text("move me")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_move_file(self):
        """Should move file to new location."""
        result = call_tool("move_file", {"source": "source.txt", "destination": "dest.txt"})
        self.assertIsNone(result["error"])
        self.assertFalse((Path(self.tmpdir) / "source.txt").exists())
        self.assertTrue((Path(self.tmpdir) / "dest.txt").exists())
        self.assertEqual((Path(self.tmpdir) / "dest.txt").read_text(), "move me")

    def test_move_creates_directories(self):
        """Should create destination directories."""
        result = call_tool("move_file", {"source": "source.txt", "destination": "new/dir/file.txt"})
        self.assertIsNone(result["error"])
        self.assertTrue((Path(self.tmpdir) / "new" / "dir" / "file.txt").exists())


class TestListDirectory(unittest.TestCase):
    """Tests for list_directory tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")
        (Path(self.tmpdir) / "file1.txt").write_text("a")
        (Path(self.tmpdir) / "file2.txt").write_text("b")
        (Path(self.tmpdir) / "subdir").mkdir()
        (Path(self.tmpdir) / ".hidden").write_text("hidden")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_list_root(self):
        """Should list root directory."""
        result = call_tool("list_directory", {"path": ""})
        self.assertIsNone(result["error"])
        self.assertIn("file: file1.txt", result["content"])
        self.assertIn("file: file2.txt", result["content"])
        self.assertIn("dir: subdir", result["content"])

    def test_list_hides_dotfiles(self):
        """Should hide dotfiles."""
        result = call_tool("list_directory", {"path": ""})
        self.assertNotIn(".hidden", result["content"])


class TestCreateDirectory(unittest.TestCase):
    """Tests for create_directory tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_create_single_directory(self):
        """Should create directory."""
        result = call_tool("create_directory", {"path": "newdir"})
        self.assertIsNone(result["error"])
        self.assertTrue((Path(self.tmpdir) / "newdir").is_dir())

    def test_create_nested_directories(self):
        """Should create nested directories."""
        result = call_tool("create_directory", {"path": "a/b/c"})
        self.assertIsNone(result["error"])
        self.assertTrue((Path(self.tmpdir) / "a" / "b" / "c").is_dir())


class TestSearch(unittest.TestCase):
    """Tests for search tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")
        (Path(self.tmpdir) / "apple.md").write_text("This is an apple")
        (Path(self.tmpdir) / "banana.md").write_text("This is a banana")
        (Path(self.tmpdir) / "docs").mkdir()
        (Path(self.tmpdir) / "docs" / "guide.md").write_text("Apple guide content")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_search_filename(self):
        """Should find files by name."""
        result = call_tool("search", {"query": "apple"})
        self.assertIsNone(result["error"])
        self.assertIn("apple.md", result["content"])

    def test_search_content(self):
        """Should find files by content."""
        result = call_tool("search", {"query": "banana"})
        self.assertIsNone(result["error"])
        self.assertIn("banana.md", result["content"])

    def test_search_case_insensitive(self):
        """Should be case insensitive."""
        result = call_tool("search", {"query": "APPLE"})
        self.assertIsNone(result["error"])
        self.assertIn("apple", result["content"].lower())


class TestSearchAdvanced(unittest.TestCase):
    """Tests for search_advanced tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")
        (Path(self.tmpdir) / "test.py").write_text("def hello():\n    pass")
        (Path(self.tmpdir) / "test.md").write_text("# Hello World")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_regex_search(self):
        """Should support regex patterns."""
        result = call_tool("search_advanced", {"pattern": r"def \w+\(\)"})
        self.assertIsNone(result["error"])
        self.assertIn("test.py", result["content"])

    def test_glob_filter(self):
        """Should filter by glob pattern."""
        result = call_tool("search_advanced", {"pattern": "Hello", "glob": "*.md"})
        self.assertIsNone(result["error"])
        self.assertIn("test.md", result["content"])
        self.assertNotIn("test.py", result["content"])


class TestFileInfo(unittest.TestCase):
    """Tests for file_info tool."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        init_security(Path(self.tmpdir), "test_token")
        (Path(self.tmpdir) / "test.txt").write_text("hello")

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmpdir)

    def test_file_info(self):
        """Should return file metadata."""
        result = call_tool("file_info", {"path": "test.txt"})
        self.assertIsNone(result["error"])
        self.assertIn("path: test.txt", result["content"])
        self.assertIn("type: file", result["content"])
        self.assertIn("size: 5", result["content"])
        self.assertIn("modified:", result["content"])


if __name__ == "__main__":
    unittest.main()
