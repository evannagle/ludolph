"""LLM proxy module using LiteLLM for multi-provider support."""

from typing import Any, Iterator

import litellm
from litellm import completion


class LlmError(Exception):
    """Base class for LLM errors."""


class LlmAuthError(LlmError):
    """Authentication failed."""


class LlmBudgetError(LlmError):
    """Budget or credits exceeded."""


class LlmRateLimitError(LlmError):
    """Rate limit exceeded."""


class LlmApiError(LlmError):
    """Generic API error."""


def chat(
    model: str,
    messages: list[dict[str, Any]],
    tools: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    """
    Send a chat request to an LLM provider via LiteLLM.

    Args:
        model: Model identifier (e.g., "claude-sonnet-4", "gpt-4o", "ollama/llama3")
        messages: List of message dicts with "role" and "content"
        tools: Optional list of tool definitions

    Returns:
        Dict with "content", "tool_calls", and "usage" keys

    Raises:
        LlmAuthError: Invalid API key or OAuth token
        LlmBudgetError: Credits exhausted
        LlmRateLimitError: Rate limited
        LlmApiError: Other API errors
    """
    try:
        kwargs: dict[str, Any] = {
            "model": model,
            "messages": messages,
        }
        if tools:
            kwargs["tools"] = tools

        response = completion(**kwargs)

        # Extract tool calls if present
        tool_calls = None
        if response.choices[0].message.tool_calls:
            tool_calls = [
                {
                    "id": tc.id,
                    "type": "function",
                    "function": {
                        "name": tc.function.name,
                        "arguments": tc.function.arguments,
                    },
                }
                for tc in response.choices[0].message.tool_calls
            ]

        return {
            "content": response.choices[0].message.content,
            "tool_calls": tool_calls,
            "usage": dict(response.usage) if hasattr(response.usage, "_asdict") else {},
        }

    except litellm.AuthenticationError as e:
        raise LlmAuthError(str(e)) from e
    except litellm.BudgetExceededError as e:
        raise LlmBudgetError(str(e)) from e
    except litellm.RateLimitError as e:
        raise LlmRateLimitError(str(e)) from e
    except litellm.APIError as e:
        raise LlmApiError(str(e)) from e


def chat_stream(
    model: str,
    messages: list[dict[str, Any]],
    tools: list[dict[str, Any]] | None = None,
) -> Iterator[dict[str, Any]]:
    """
    Stream a chat request, yielding chunks as they arrive.

    Args:
        model: Model identifier (e.g., "claude-sonnet-4", "gpt-4o", "ollama/llama3")
        messages: List of message dicts with "role" and "content"
        tools: Optional list of tool definitions

    Yields:
        Dict with "content" and/or "tool_calls" for each chunk

    Raises:
        LlmAuthError: Invalid API key or OAuth token
        LlmBudgetError: Credits exhausted
        LlmRateLimitError: Rate limited
        LlmApiError: Other API errors
    """
    try:
        kwargs: dict[str, Any] = {
            "model": model,
            "messages": messages,
            "stream": True,
        }
        if tools:
            kwargs["tools"] = tools

        response = completion(**kwargs)

        for chunk in response:
            if not chunk.choices:
                continue

            delta = chunk.choices[0].delta

            yield {
                "content": delta.content,
                "tool_calls": None,
            }

    except litellm.AuthenticationError as e:
        raise LlmAuthError(str(e)) from e
    except litellm.BudgetExceededError as e:
        raise LlmBudgetError(str(e)) from e
    except litellm.RateLimitError as e:
        raise LlmRateLimitError(str(e)) from e
    except litellm.APIError as e:
        raise LlmApiError(str(e)) from e
