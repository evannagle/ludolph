"""Tests for LLM proxy module."""

from unittest.mock import MagicMock, patch

import pytest


def test_chat_returns_response():
    """Chat endpoint returns content from LiteLLM."""
    from llm import chat

    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = "Hello!"
    mock_response.choices[0].message.tool_calls = None
    mock_response.usage = MagicMock()
    mock_response.usage._asdict = lambda: {"prompt_tokens": 10, "completion_tokens": 5}

    with patch("llm.completion", return_value=mock_response):
        result = chat(
            model="claude-sonnet-4",
            messages=[{"role": "user", "content": "Hi"}],
        )

    assert result["content"] == "Hello!"
    assert result["tool_calls"] is None
    assert "usage" in result


def test_chat_handles_tool_calls():
    """Chat returns tool_calls when present."""
    from llm import chat

    mock_tool_call = MagicMock()
    mock_tool_call.id = "call_123"
    mock_tool_call.function.name = "read_file"
    mock_tool_call.function.arguments = '{"path": "test.md"}'

    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = None
    mock_response.choices[0].message.tool_calls = [mock_tool_call]
    mock_response.usage = MagicMock()
    mock_response.usage._asdict = lambda: {"prompt_tokens": 10, "completion_tokens": 5}

    with patch("llm.completion", return_value=mock_response):
        result = chat(
            model="claude-sonnet-4",
            messages=[{"role": "user", "content": "Read test.md"}],
            tools=[{"type": "function", "function": {"name": "read_file"}}],
        )

    assert result["content"] is None
    assert len(result["tool_calls"]) == 1
    assert result["tool_calls"][0]["id"] == "call_123"


def test_chat_raises_on_auth_error():
    """Chat raises appropriate error on authentication failure."""
    import litellm

    from llm import LlmAuthError, chat

    with patch("llm.completion", side_effect=litellm.AuthenticationError(
        message="Invalid API key",
        llm_provider="anthropic",
        model="claude-sonnet-4",
    )), pytest.raises(LlmAuthError):
        chat(model="claude-sonnet-4", messages=[{"role": "user", "content": "Hi"}])


def test_chat_raises_on_budget_exceeded():
    """Chat raises appropriate error when budget is exceeded."""
    import litellm

    from llm import LlmBudgetError, chat

    with patch("llm.completion", side_effect=litellm.BudgetExceededError(
        message="Budget exceeded",
        current_cost=100.0,
        max_budget=50.0,
    )), pytest.raises(LlmBudgetError):
        chat(model="claude-sonnet-4", messages=[{"role": "user", "content": "Hi"}])


def test_chat_raises_on_rate_limit():
    """Chat raises appropriate error when rate limited."""
    import litellm

    from llm import LlmRateLimitError, chat

    with patch("llm.completion", side_effect=litellm.RateLimitError(
        message="Rate limit exceeded",
        llm_provider="anthropic",
        model="claude-sonnet-4",
    )), pytest.raises(LlmRateLimitError):
        chat(model="claude-sonnet-4", messages=[{"role": "user", "content": "Hi"}])


def test_chat_raises_on_api_error():
    """Chat raises appropriate error on generic API failures."""
    import litellm

    from llm import LlmApiError, chat

    with patch("llm.completion", side_effect=litellm.APIError(
        message="API error",
        llm_provider="anthropic",
        model="claude-sonnet-4",
        status_code=500,
    )), pytest.raises(LlmApiError):
        chat(model="claude-sonnet-4", messages=[{"role": "user", "content": "Hi"}])
