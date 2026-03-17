"""Tests for conversation philosophy context in chat endpoint."""

import json
import os
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

# Add mcp directory to path so imports match server.py's import style
sys.path.insert(0, str(Path(__file__).parent.parent))


@pytest.fixture
def client():
    """Create test client with auth configured."""
    os.environ["VAULT_PATH"] = "/tmp/test-vault"
    os.environ["AUTH_TOKEN"] = "test-token"

    # Use local imports to match server.py's import style
    import security
    from server import app

    Path("/tmp/test-vault").mkdir(exist_ok=True)
    security.init_security(Path("/tmp/test-vault"), "test-token")

    app.config["TESTING"] = True
    with app.test_client() as client:
        yield client


def test_chat_includes_conversation_principles(client):
    """Chat endpoint should inject conversation principles into system prompt."""
    with patch("server.llm_chat") as mock_llm:
        mock_llm.return_value = {"content": "hi", "tool_calls": None, "usage": {}}

        response = client.post(
            "/chat",
            json={
                "messages": [
                    {"role": "system", "content": "You are Lu."},
                    {"role": "user", "content": "hello"}
                ]
            },
            headers={"Authorization": "Bearer test-token"}
        )

        assert response.status_code == 200

        call_args = mock_llm.call_args
        messages = call_args.kwargs.get("messages", call_args[1].get("messages", []))
        system_msg = next((m for m in messages if m.get("role") == "system"), None)

        assert system_msg is not None
        assert "CONVERSATION PRINCIPLES" in system_msg["content"]
        assert "Scoping" in system_msg["content"]
        assert "Pacing" in system_msg["content"]
        assert "Ma" in system_msg["content"]


def test_chat_creates_system_message_if_none_exists(client):
    """Chat should create system message with principles if none exists."""
    with patch("server.llm_chat") as mock_llm:
        mock_llm.return_value = {"content": "hi", "tool_calls": None, "usage": {}}

        response = client.post(
            "/chat",
            json={
                "messages": [
                    {"role": "user", "content": "hello"}
                ]
            },
            headers={"Authorization": "Bearer test-token"}
        )

        assert response.status_code == 200

        call_args = mock_llm.call_args
        messages = call_args.kwargs.get("messages", call_args[1].get("messages", []))
        system_msg = next((m for m in messages if m.get("role") == "system"), None)

        assert system_msg is not None
        assert "CONVERSATION PRINCIPLES" in system_msg["content"]


def test_chat_preserves_original_system_content(client):
    """Chat should preserve original system prompt content while adding principles."""
    with patch("server.llm_chat") as mock_llm:
        mock_llm.return_value = {"content": "hi", "tool_calls": None, "usage": {}}

        response = client.post(
            "/chat",
            json={
                "messages": [
                    {"role": "system", "content": "You are Lu, a helpful assistant."},
                    {"role": "user", "content": "hello"}
                ]
            },
            headers={"Authorization": "Bearer test-token"}
        )

        assert response.status_code == 200

        call_args = mock_llm.call_args
        messages = call_args.kwargs.get("messages", call_args[1].get("messages", []))
        system_msg = next((m for m in messages if m.get("role") == "system"), None)

        assert system_msg is not None
        assert "You are Lu, a helpful assistant." in system_msg["content"]
        assert "CONVERSATION PRINCIPLES" in system_msg["content"]


@pytest.fixture
def app_client(tmp_path):
    """Create test client with auth configured using tmp_path for vault."""
    os.environ["VAULT_PATH"] = str(tmp_path)
    os.environ["AUTH_TOKEN"] = "test-token"

    # Use local imports to match server.py's import style
    import security
    from server import app

    security.init_security(tmp_path, "test-token")

    app.config["TESTING"] = True
    with app.test_client() as client:
        yield client, tmp_path


def test_chat_includes_open_topics(app_client):
    """Chat endpoint should include open topics when user_id provided."""
    import context

    client, tmp_path = app_client

    conv_dir = tmp_path / ".lu" / "conversations"
    conv_dir.mkdir(parents=True)
    state_file = conv_dir / "123.json"
    state_file.write_text(json.dumps({
        "id": "123",
        "topics": ["Work project", "Dinner plans"],
        "current": "Work project"
    }))

    with patch("server.llm_chat") as mock_llm, \
         patch.object(context, "get_vault_path", return_value=tmp_path):
        mock_llm.return_value = {"content": "hi", "tool_calls": None, "usage": {}}

        response = client.post(
            "/chat",
            json={
                "messages": [{"role": "user", "content": "hello"}],
                "user_id": 123
            },
            headers={"Authorization": "Bearer test-token"}
        )

        assert response.status_code == 200

        call_args = mock_llm.call_args
        messages = call_args.kwargs.get("messages", [])
        system_msg = next((m for m in messages if m.get("role") == "system"), None)

        assert system_msg is not None
        assert "Work project" in system_msg["content"]
        assert "Open topics" in system_msg["content"]
