"""Tests for /chat endpoint."""

import os
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

# Add parent to path for package imports
sys.path.insert(0, str(Path(__file__).parent.parent.parent))


@pytest.fixture
def client():
    """Create test client with auth configured."""
    os.environ["VAULT_PATH"] = "/tmp/test-vault"
    os.environ["AUTH_TOKEN"] = "test-token"

    from mcp.security import init_security
    from mcp.server import app

    Path("/tmp/test-vault").mkdir(exist_ok=True)
    init_security(Path("/tmp/test-vault"), "test-token")

    app.config["TESTING"] = True
    with app.test_client() as client:
        yield client


def test_chat_requires_auth(client):
    """Chat endpoint requires authentication."""
    response = client.post("/chat", json={
        "model": "claude-sonnet-4",
        "messages": [{"role": "user", "content": "Hi"}],
    })
    assert response.status_code == 401


def test_chat_returns_response(client):
    """Chat endpoint returns LLM response."""
    with patch("mcp.server.llm_chat", return_value={
        "content": "Hello!",
        "tool_calls": None,
        "usage": {"prompt_tokens": 10, "completion_tokens": 5},
    }):
        response = client.post(
            "/chat",
            json={
                "model": "claude-sonnet-4",
                "messages": [{"role": "user", "content": "Hi"}],
            },
            headers={"Authorization": "Bearer test-token"},
        )

    assert response.status_code == 200
    data = response.get_json()
    assert data["content"] == "Hello!"


def test_chat_returns_401_on_auth_error(client):
    """Chat returns 401 on authentication error."""
    from mcp.llm import LlmAuthError

    with patch("mcp.server.llm_chat", side_effect=LlmAuthError("Invalid key")):
        response = client.post(
            "/chat",
            json={
                "model": "claude-sonnet-4",
                "messages": [{"role": "user", "content": "Hi"}],
            },
            headers={"Authorization": "Bearer test-token"},
        )

    assert response.status_code == 401
    data = response.get_json()
    assert data["error"] == "auth_failed"


def test_chat_returns_402_on_budget_error(client):
    """Chat returns 402 when credits exhausted."""
    from mcp.llm import LlmBudgetError

    with patch("mcp.server.llm_chat", side_effect=LlmBudgetError("Credits exhausted")):
        response = client.post(
            "/chat",
            json={
                "model": "claude-sonnet-4",
                "messages": [{"role": "user", "content": "Hi"}],
            },
            headers={"Authorization": "Bearer test-token"},
        )

    assert response.status_code == 402
    data = response.get_json()
    assert data["error"] == "budget_exceeded"


def test_chat_returns_429_on_rate_limit(client):
    """Chat returns 429 on rate limit."""
    from mcp.llm import LlmRateLimitError

    with patch("mcp.server.llm_chat", side_effect=LlmRateLimitError("Rate limited")):
        response = client.post(
            "/chat",
            json={
                "model": "claude-sonnet-4",
                "messages": [{"role": "user", "content": "Hi"}],
            },
            headers={"Authorization": "Bearer test-token"},
        )

    assert response.status_code == 429
    data = response.get_json()
    assert data["error"] == "rate_limit"


def test_chat_returns_502_on_api_error(client):
    """Chat returns 502 on generic API error."""
    from mcp.llm import LlmApiError

    with patch("mcp.server.llm_chat", side_effect=LlmApiError("API error")):
        response = client.post(
            "/chat",
            json={
                "model": "claude-sonnet-4",
                "messages": [{"role": "user", "content": "Hi"}],
            },
            headers={"Authorization": "Bearer test-token"},
        )

    assert response.status_code == 502
    data = response.get_json()
    assert data["error"] == "api_error"


def test_chat_rejects_empty_messages(client):
    """Chat returns 400 when messages is empty."""
    response = client.post(
        "/chat",
        json={
            "model": "claude-sonnet-4",
            "messages": [],
        },
        headers={"Authorization": "Bearer test-token"},
    )

    assert response.status_code == 400
    data = response.get_json()
    assert data["error"] == "invalid_input"


def test_chat_rejects_missing_messages(client):
    """Chat returns 400 when messages is missing."""
    response = client.post(
        "/chat",
        json={
            "model": "claude-sonnet-4",
        },
        headers={"Authorization": "Bearer test-token"},
    )

    assert response.status_code == 400
    data = response.get_json()
    assert data["error"] == "invalid_input"


def test_chat_stream_returns_sse(client):
    """Chat stream returns Server-Sent Events."""
    with patch("mcp.server.llm_chat_stream", return_value=iter([
        {"content": "Hello", "tool_calls": None},
        {"content": " world", "tool_calls": None},
    ])):
        response = client.post(
            "/chat/stream",
            json={"model": "claude-sonnet-4", "messages": [{"role": "user", "content": "Hi"}]},
            headers={"Authorization": "Bearer test-token"},
        )

    assert response.status_code == 200
    assert response.content_type == "text/event-stream; charset=utf-8"
    data = response.data.decode("utf-8")
    assert 'data: {"content": "Hello"' in data
    assert "[DONE]" in data
