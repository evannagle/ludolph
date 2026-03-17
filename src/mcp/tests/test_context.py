"""Tests for context loading module."""

import sys
from pathlib import Path
from unittest.mock import patch

import pytest

# Add mcp directory to path so imports match server.py's import style
sys.path.insert(0, str(Path(__file__).parent.parent))


def test_load_philosophy_returns_file_content(tmp_path):
    """load_philosophy returns content from .lu/philosophy.md."""
    import context

    lu_dir = tmp_path / ".lu"
    lu_dir.mkdir()
    philosophy_file = lu_dir / "philosophy.md"
    philosophy_file.write_text("# Custom Philosophy\n\nBe excellent.")

    with patch("context.get_vault_path", return_value=tmp_path):
        result = context.load_philosophy()

    assert result is not None
    assert "Custom Philosophy" in result
    assert "Be excellent" in result


def test_load_philosophy_creates_default_if_missing(tmp_path):
    """load_philosophy creates default file if missing."""
    import context

    with patch("context.get_vault_path", return_value=tmp_path):
        result = context.load_philosophy()

    philosophy_file = tmp_path / ".lu" / "philosophy.md"
    assert philosophy_file.exists()
    assert "Scoping" in result
    assert "Pacing" in result
    assert "Ma" in result


def test_load_philosophy_handles_read_error(tmp_path):
    """load_philosophy returns None on read errors."""
    import context

    lu_dir = tmp_path / ".lu"
    lu_dir.mkdir()
    (lu_dir / "philosophy.md").mkdir()  # Directory, not file - causes error

    with patch("context.get_vault_path", return_value=tmp_path):
        result = context.load_philosophy()

    assert result is None


def test_inject_principles_includes_philosophy(tmp_path):
    """inject_principles includes philosophy file content."""
    import context

    lu_dir = tmp_path / ".lu"
    lu_dir.mkdir()
    (lu_dir / "philosophy.md").write_text("# My Philosophy\n\nBe kind.")

    messages = [{"role": "user", "content": "hello"}]

    with patch.object(context, "get_vault_path", return_value=tmp_path):
        result = context.inject_principles(messages)

    system_msg = result[0]
    assert system_msg["role"] == "system"
    assert "CONVERSATION PRINCIPLES" in system_msg["content"]
    assert "My Philosophy" in system_msg["content"]
    assert "Be kind" in system_msg["content"]
