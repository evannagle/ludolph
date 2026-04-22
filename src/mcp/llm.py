"""LLM proxy module using LiteLLM for multi-provider support.

The ``chat()`` function streams internally and captures every token to a
scratch file as it arrives. If the connection dies mid-generation, the
partial content is preserved and returned via ``LlmTimeoutError.partial_content``,
so no work is lost when Anthropic hangs up on us.
"""

import logging
import os
import time
import uuid
from pathlib import Path
from typing import Any, Iterator

import litellm
from litellm import completion

logger = logging.getLogger(__name__)

# Maximum seconds to wait for a single LLM completion before giving up.
# Long tool-call responses (e.g. writing 2k+ words to a file) can take
# 2-3 minutes of streaming.  300s avoids premature timeouts while still
# catching truly dead connections.
LLM_REQUEST_TIMEOUT = 300

# Max seconds to wait between streaming chunks before treating the
# connection as stalled. Claude sometimes pauses 30-60s mid-generation
# on complex or lengthy content; 120s catches genuinely dead streams
# without false-firing on legitimate pauses.
LLM_STREAM_STALL_TIMEOUT = 120

# Number of automatic retries on transient failures (timeouts, connection errors).
# Set to 1 so the user doesn't wait through multiple 120s timeouts on a dead line.
LLM_MAX_RETRIES = 1

# Where partial responses get saved as they stream. On timeout or error,
# the caller can read the scratch file or the exception's partial_content
# attribute to recover what was generated before the failure.
SCRATCH_DIR = Path.home() / ".ludolph" / "scratch"

# Delete scratch files older than this many hours.
SCRATCH_RETENTION_HOURS = 48


class LlmError(Exception):
    """Base class for LLM errors."""


class LlmKeyMissingError(LlmError):
    """API key not configured."""


class LlmAuthError(LlmError):
    """Authentication failed (invalid key)."""


class LlmBudgetError(LlmError):
    """Budget or credits exceeded."""


class LlmRateLimitError(LlmError):
    """Rate limit exceeded."""


class LlmTimeoutError(LlmError):
    """Request timed out — connection stalled or response took too long.

    Carries any partial content captured from the stream before the
    timeout, so callers can recover the work that was already generated.
    """

    def __init__(
        self,
        message: str,
        partial_content: str = "",
        scratch_path: str | None = None,
    ):
        super().__init__(message)
        self.partial_content = partial_content
        self.scratch_path = scratch_path


class LlmApiError(LlmError):
    """Generic API error.

    Can also carry partial content if the error occurred after some
    streaming had already taken place.
    """

    def __init__(
        self,
        message: str,
        partial_content: str = "",
        scratch_path: str | None = None,
    ):
        super().__init__(message)
        self.partial_content = partial_content
        self.scratch_path = scratch_path


def _ensure_scratch_dir() -> Path:
    """Create the scratch directory if missing and prune old files."""
    SCRATCH_DIR.mkdir(parents=True, exist_ok=True)

    # Prune stale scratch files opportunistically
    cutoff = time.time() - (SCRATCH_RETENTION_HOURS * 3600)
    try:
        for entry in SCRATCH_DIR.iterdir():
            if entry.is_file() and entry.stat().st_mtime < cutoff:
                try:
                    entry.unlink()
                except OSError:
                    pass
    except OSError:
        pass

    return SCRATCH_DIR


def _new_scratch_path() -> Path:
    """Generate a unique scratch file path for a single LLM request."""
    _ensure_scratch_dir()
    return SCRATCH_DIR / f"chat-{int(time.time())}-{uuid.uuid4().hex[:8]}.txt"


def _stream_to_scratch(
    kwargs: dict[str, Any],
    scratch_path: Path,
) -> dict[str, Any]:
    """Stream a completion, writing tokens to scratch file as they arrive.

    Returns a result dict with content, tool_calls, and usage. Raises
    LlmTimeoutError with partial_content populated if the stream stalls
    or the overall timeout fires.
    """
    content_parts: list[str] = []
    tool_calls_by_index: dict[int, dict[str, Any]] = {}
    usage: dict[str, Any] = {}

    # Open scratch file and stream tokens into it as they arrive.
    # flush() after every chunk so the file is usable if the process dies.
    stream_kwargs = dict(kwargs)
    stream_kwargs["stream"] = True
    # Ask LiteLLM for usage in the stream final chunk (OpenAI-compatible option)
    stream_kwargs["stream_options"] = {"include_usage": True}

    scratch_file = scratch_path.open("w", encoding="utf-8")
    last_chunk_time = time.time()

    try:
        response = completion(**stream_kwargs)

        for chunk in response:
            # Stall detection: if too long passes between chunks, treat as timeout
            now = time.time()
            if now - last_chunk_time > LLM_STREAM_STALL_TIMEOUT:
                raise litellm.Timeout(
                    message=f"No chunks received for {LLM_STREAM_STALL_TIMEOUT}s",
                    model=kwargs["model"],
                    llm_provider="anthropic",
                )
            last_chunk_time = now

            # Capture usage from the final chunk when LiteLLM surfaces it
            chunk_usage = getattr(chunk, "usage", None)
            if chunk_usage is not None:
                try:
                    usage = dict(chunk_usage) if hasattr(chunk_usage, "_asdict") else {
                        "prompt_tokens": getattr(chunk_usage, "prompt_tokens", 0),
                        "completion_tokens": getattr(chunk_usage, "completion_tokens", 0),
                        "total_tokens": getattr(chunk_usage, "total_tokens", 0),
                    }
                except Exception:  # noqa: BLE001
                    usage = {}

            if not chunk.choices:
                continue

            delta = chunk.choices[0].delta

            # Text content chunk
            if delta.content:
                content_parts.append(delta.content)
                scratch_file.write(delta.content)
                scratch_file.flush()

            # Tool call deltas — these arrive incrementally and must be
            # reassembled per index
            delta_tool_calls = getattr(delta, "tool_calls", None) or []
            for tc_delta in delta_tool_calls:
                index = getattr(tc_delta, "index", 0) or 0
                existing = tool_calls_by_index.setdefault(
                    index,
                    {
                        "id": None,
                        "type": "function",
                        "function": {"name": "", "arguments": ""},
                    },
                )
                if getattr(tc_delta, "id", None):
                    existing["id"] = tc_delta.id
                fn_delta = getattr(tc_delta, "function", None)
                if fn_delta is not None:
                    if getattr(fn_delta, "name", None):
                        existing["function"]["name"] = fn_delta.name
                    if getattr(fn_delta, "arguments", None):
                        existing["function"]["arguments"] += fn_delta.arguments

        content = "".join(content_parts) if content_parts else None
        tool_calls = (
            [tool_calls_by_index[i] for i in sorted(tool_calls_by_index.keys())]
            if tool_calls_by_index
            else None
        )

        return {"content": content, "tool_calls": tool_calls, "usage": usage}
    finally:
        try:
            scratch_file.close()
        except Exception:  # noqa: BLE001
            pass


def chat(
    model: str,
    messages: list[dict[str, Any]],
    tools: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    """
    Send a chat request to an LLM provider via LiteLLM.

    Streams internally, writing tokens to a scratch file as they arrive,
    so partial content is recoverable if the connection dies.

    Args:
        model: Model identifier (e.g., "claude-sonnet-4", "gpt-4o", "ollama/llama3")
        messages: List of message dicts with "role" and "content"
        tools: Optional list of tool definitions

    Returns:
        Dict with "content", "tool_calls", "usage", and "scratch_path" keys.
        scratch_path points to a file containing the raw streamed text — it's
        removed on success and kept on failure.

    Raises:
        LlmKeyMissingError: API key not set
        LlmAuthError: Invalid API key or OAuth token
        LlmBudgetError: Credits exhausted
        LlmRateLimitError: Rate limited
        LlmTimeoutError: Request stalled — partial_content attribute carries
            whatever was generated before the timeout
        LlmApiError: Other API errors
    """
    api_key = os.environ.get("ANTHROPIC_API_KEY", "")
    if not api_key:
        raise LlmKeyMissingError("ANTHROPIC_API_KEY not set. Run `lu setup mcp` to configure.")

    kwargs: dict[str, Any] = {
        "model": model,
        "messages": messages,
        "timeout": LLM_REQUEST_TIMEOUT,
    }
    if tools:
        kwargs["tools"] = tools

    # Retry loop for transient failures. Auth/budget/rate-limit errors are
    # not retried — they won't succeed on retry.
    last_error: Exception | None = None
    last_partial = ""
    last_scratch_path: Path | None = None

    for attempt in range(LLM_MAX_RETRIES + 1):
        scratch_path = _new_scratch_path()
        last_scratch_path = scratch_path

        try:
            result = _stream_to_scratch(kwargs, scratch_path)
            # Success — remove the scratch file to keep the dir clean
            try:
                scratch_path.unlink(missing_ok=True)
            except OSError:
                pass
            result["scratch_path"] = None
            return result

        except litellm.AuthenticationError as e:
            raise LlmAuthError(str(e)) from e
        except litellm.BudgetExceededError as e:
            raise LlmBudgetError(str(e)) from e
        except litellm.RateLimitError as e:
            raise LlmRateLimitError(str(e)) from e
        except litellm.BadRequestError as e:
            raise LlmApiError(f"Invalid request: {e}") from e
        except litellm.Timeout as e:
            last_error = e
            last_partial = _read_scratch(scratch_path)
            logger.warning(
                "LLM stream timed out (attempt %d/%d, captured %d chars)",
                attempt + 1, LLM_MAX_RETRIES + 1, len(last_partial),
            )
            # Retry only if we haven't captured ANY content. If we already
            # got partial output, treat it as worth preserving rather than
            # starting over.
            if attempt < LLM_MAX_RETRIES and not last_partial:
                time.sleep(1)
                continue
            raise LlmTimeoutError(
                f"LLM stream stalled after {LLM_REQUEST_TIMEOUT}s. "
                f"{'Partial content was recovered.' if last_partial else 'No content was generated.'}",
                partial_content=last_partial,
                scratch_path=str(scratch_path) if last_partial else None,
            ) from e
        except litellm.APIConnectionError as e:
            last_error = e
            last_partial = _read_scratch(scratch_path)
            logger.warning(
                "LLM connection error (attempt %d/%d): %s",
                attempt + 1, LLM_MAX_RETRIES + 1, e,
            )
            if attempt < LLM_MAX_RETRIES and not last_partial:
                time.sleep(1)
                continue
            raise LlmApiError(
                f"Connection error: {e}",
                partial_content=last_partial,
                scratch_path=str(scratch_path) if last_partial else None,
            ) from e
        except litellm.APIError as e:
            partial = _read_scratch(scratch_path)
            raise LlmApiError(
                str(e),
                partial_content=partial,
                scratch_path=str(scratch_path) if partial else None,
            ) from e

    # Unreachable but keeps type checkers happy
    raise LlmApiError(
        f"Exhausted retries: {last_error}",
        partial_content=last_partial,
        scratch_path=str(last_scratch_path) if last_partial else None,
    )


def _read_scratch(path: Path) -> str:
    """Read a scratch file, returning empty string on any failure."""
    try:
        return path.read_text(encoding="utf-8") if path.exists() else ""
    except OSError:
        return ""


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
            "timeout": LLM_REQUEST_TIMEOUT,
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
    except litellm.BadRequestError as e:
        raise LlmApiError(f"Invalid request: {e}") from e
    except litellm.Timeout as e:
        raise LlmTimeoutError(
            f"LLM stream timed out after {LLM_REQUEST_TIMEOUT}s. "
            f"The connection stalled — your work was not saved."
        ) from e
    except litellm.APIError as e:
        raise LlmApiError(str(e)) from e
