"""Integration tests for conversation philosophy system.

Tests that all components work together: core principles, philosophy file,
and open topics all appear in the system message when a chat request includes
a user_id. This ensures Claude receives full user context for personalized
conversation behavior.
"""

import json
import os
import sys
from pathlib import Path
from unittest.mock import patch, MagicMock

import pytest

# Add mcp directory to path so imports match server.py's import style
sys.path.insert(0, str(Path(__file__).parent.parent))


@pytest.fixture
def app_client_with_philosophy(tmp_path):
    """Create test client with vault containing philosophy and topics."""
    # Set up vault structure
    lu_dir = tmp_path / ".lu"
    lu_dir.mkdir()

    # Custom philosophy file
    (lu_dir / "philosophy.md").write_text("# My Style\nBe excellent.")

    # User conversation state with topics
    conv_dir = lu_dir / "conversations"
    conv_dir.mkdir()
    (conv_dir / "456.json").write_text(
        json.dumps(
            {
                "id": "456",
                "topics": ["Build feature", "Fix bug"],
                "current": "Build feature",
                "updated": "2026-03-16T12:00:00Z",
            }
        )
    )

    # Configure environment
    os.environ["VAULT_PATH"] = str(tmp_path)
    os.environ["AUTH_TOKEN"] = "test-token"

    # Import and initialize after env is set
    import security
    from server import app

    security.init_security(tmp_path, "test-token")

    app.config["TESTING"] = True
    with app.test_client() as client:
        yield client, tmp_path


def test_full_philosophy_context_flow(app_client_with_philosophy):
    """
    Integration test: Full flow from request to LLM call.

    Verifies that when a chat request includes a user_id:
    1. Core principles (Scoping, Pacing, Ma) are injected
    2. Philosophy file content is loaded and included
    3. User's open topics are included in context

    This ensures Claude has all the context needed to maintain Lu's
    conversation philosophy with each specific user.
    """
    import context

    client, tmp_path = app_client_with_philosophy

    with (
        patch("server.llm_chat") as mock_llm,
        patch.object(context, "get_vault_path", return_value=tmp_path),
    ):
        mock_llm.return_value = {"content": "Got it!", "tool_calls": None, "usage": {}}

        response = client.post(
            "/chat",
            json={
                "messages": [
                    {"role": "system", "content": "You are Lu."},
                    {"role": "user", "content": "What's on my list?"},
                ],
                "user_id": 456,
            },
            headers={"Authorization": "Bearer test-token"},
        )

    # Verify request succeeded
    assert response.status_code == 200

    # Verify LLM was called
    assert mock_llm.called, "llm_chat should have been called"

    # Extract messages from the LLM call
    call_args = mock_llm.call_args
    messages = call_args.kwargs.get("messages", call_args[1].get("messages", []))

    # Find system message
    system_msg = next((m for m in messages if m.get("role") == "system"), None)
    assert system_msg is not None, "System message should exist"

    content = system_msg["content"]

    # 1. Core principles present
    assert "CONVERSATION PRINCIPLES" in content, "Core principles header missing"
    assert "Scoping:" in content, "Scoping principle missing"
    assert "Pacing:" in content, "Pacing principle missing"
    assert "Ma:" in content, "Ma principle missing"

    # 2. Philosophy file content present
    assert "My Style" in content, "Philosophy file content missing"
    assert "Be excellent" in content, "Philosophy file content missing"

    # 3. User topics present
    assert "Build feature" in content, "User topic 'Build feature' missing"
    assert "Fix bug" in content, "User topic 'Fix bug' missing"
    assert "Open topics" in content, "Open topics section missing"
