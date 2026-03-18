# src/mcp/tests/test_conversation.py
"""Tests for conversation scope tool."""

import json
import os
import sys
import pytest
from datetime import datetime, timezone, timedelta
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def test_expire_stale_topics_moves_old_topics(tmp_path):
    """expire_stale_topics moves topics older than max_age to stale."""
    from tools import conversation

    conv_dir = tmp_path / ".lu" / "conversations"
    conv_dir.mkdir(parents=True)

    old_time = (datetime.now(timezone.utc) - timedelta(hours=25)).isoformat()
    state_file = conv_dir / "user_123.json"
    state_file.write_text(
        json.dumps(
            {
                "id": "user_123",
                "updated": old_time,
                "topics": ["Old topic"],
                "resolved": [],
                "current": "Old topic",
            }
        )
    )

    with patch.object(conversation, "get_vault_path", return_value=tmp_path):
        count = conversation.expire_stale_topics("user_123", max_age_hours=24)

    assert count == 1

    new_state = json.loads(state_file.read_text())
    assert "Old topic" not in new_state["topics"]
    assert "Old topic" in new_state.get("stale", [])


def test_expire_stale_topics_keeps_recent_topics(tmp_path):
    """expire_stale_topics keeps topics updated recently."""
    from tools import conversation

    conv_dir = tmp_path / ".lu" / "conversations"
    conv_dir.mkdir(parents=True)

    recent_time = datetime.now(timezone.utc).isoformat()
    state_file = conv_dir / "user_123.json"
    state_file.write_text(
        json.dumps(
            {
                "id": "user_123",
                "updated": recent_time,
                "topics": ["Recent topic"],
                "resolved": [],
                "current": "Recent topic",
            }
        )
    )

    with patch.object(conversation, "get_vault_path", return_value=tmp_path):
        count = conversation.expire_stale_topics("user_123", max_age_hours=24)

    assert count == 0

    new_state = json.loads(state_file.read_text())
    assert "Recent topic" in new_state["topics"]


def test_expire_stale_topics_handles_missing_file(tmp_path):
    """expire_stale_topics returns 0 for missing conversation file."""
    from tools import conversation

    with patch.object(conversation, "get_vault_path", return_value=tmp_path):
        count = conversation.expire_stale_topics("nonexistent")

    assert count == 0
