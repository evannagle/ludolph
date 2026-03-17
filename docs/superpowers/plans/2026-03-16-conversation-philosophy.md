# Conversation Philosophy Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Embed Lu's conversation philosophy (scoping, pacing, Ma) into the system through prompts, files, and memory integration.

**Architecture:** Layered context loading on Mac MCP server. Core principles go in system prompt, detailed guidance loads from `.lu/philosophy.md` (auto-created), open topics tracked in vault JSON files via `conversation_scope` tool.

**Tech Stack:** Python (Flask MCP server), existing conversation_scope tool, vault file storage

**Notes:**
- The spec mentioned modifying `src/setup.rs` for setup wizard principles - this is already done (conversation pacing added in earlier work).
- The spec mentioned `src/mcp/llm.py` but the correct approach is injecting context in `server.py` before calling llm.py.
- Lu.md user preferences are already loaded by `src/llm.rs` (Rust side) and included in the system prompt. No changes needed here.

---

## File Structure

| File | Responsibility |
|------|----------------|
| `src/mcp/context.py` | New: Context loading helpers (philosophy file, topics) |
| `src/mcp/server.py` | Modify: Add philosophy context to chat endpoint |
| `src/mcp/tools/conversation.py` | Modify: Add stale topic expiration |
| `src/mcp/tests/test_context.py` | New: Unit tests for context loading |
| `src/mcp/tests/test_conversation.py` | New: Unit tests for conversation tool |

---

## Chunk 1: Core Principles in System Prompt

### Task 1: Add Conversation Principles to Chat Endpoint

**Files:**
- Create: `src/mcp/context.py`
- Modify: `src/mcp/server.py:423-455` (chat endpoint)
- Create: `src/mcp/tests/test_server_context.py`

- [ ] **Step 1: Write the failing test**

Create test file for server chat with context:

```python
# src/mcp/tests/test_server_context.py
"""Tests for conversation philosophy context in chat endpoint."""

import os
import sys
import pytest
from unittest.mock import patch, MagicMock

# Add parent directory to path for imports
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


@pytest.fixture
def app_client(tmp_path):
    """Create test client with security mocked."""
    # Set required env vars before importing
    os.environ["AUTH_TOKEN"] = "test-token"
    os.environ["VAULT_PATH"] = str(tmp_path)

    # Import security first to initialize it
    import security
    security.init_security(tmp_path, "test-token")

    from server import app
    app.config["TESTING"] = True
    return app.test_client(), tmp_path


def test_chat_includes_conversation_principles(app_client):
    """Chat endpoint should inject conversation principles into system prompt."""
    client, tmp_path = app_client

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

        # Check that llm_chat was called with modified system prompt
        call_args = mock_llm.call_args
        messages = call_args.kwargs.get("messages", [])
        system_msg = next((m for m in messages if m.get("role") == "system"), None)

        assert system_msg is not None
        assert "CONVERSATION PRINCIPLES" in system_msg["content"]
        assert "Scoping" in system_msg["content"]
        assert "Pacing" in system_msg["content"]
        assert "Ma" in system_msg["content"]
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_server_context.py -v`
Expected: FAIL - no CONVERSATION PRINCIPLES in system message

- [ ] **Step 3: Create context module with core principles**

```python
# src/mcp/context.py
"""
Context loading for Lu's conversation philosophy.

This module handles injecting conversation principles, philosophy files,
and open topics into LLM chat context. The layered system works as follows:

1. CORE_PRINCIPLES: Always-on guidance for scoping, pacing, and Ma
2. Philosophy file: User-customizable guidance from .lu/philosophy.md
3. Open topics: Current conversation state from .lu/conversations/{user_id}.json

Usage:
    from context import inject_principles

    messages = inject_principles(messages, user_id="123")
"""

CORE_PRINCIPLES = """
CONVERSATION PRINCIPLES:

Scoping: When a message contains multiple topics or questions, silently
note them using conversation_scope, then address one at a time. Don't
announce the structure - just naturally work through them without losing
track.

Pacing: Ask one question per message. Wait for the response before asking
the next. Acknowledge what the user said before moving on.

Ma: Not every response needs to advance an agenda. Sometimes notice
something without acting on it. Sometimes appreciate a moment before
rushing forward. Read the user's energy - if they're reflective, be
reflective. If task-focused, stay efficient.
"""


def inject_principles(messages: list, user_id: str | None = None) -> list:
    """
    Inject conversation principles into the system message.

    If a system message exists, appends principles to it.
    If no system message exists, creates one with principles.

    Args:
        messages: Original message list
        user_id: Optional user ID for loading topics (used in later tasks)

    Returns a new list (does not mutate input).
    """
    result = []
    found_system = False

    for msg in messages:
        if msg.get("role") == "system" and not found_system:
            found_system = True
            result.append({
                "role": "system",
                "content": msg.get("content", "") + "\n\n" + CORE_PRINCIPLES.strip()
            })
        else:
            result.append(msg)

    if not found_system:
        result.insert(0, {"role": "system", "content": CORE_PRINCIPLES.strip()})

    return result
```

- [ ] **Step 4: Update chat endpoint to use inject_principles**

In `src/mcp/server.py`, add import at top (after other imports around line 42):

```python
from context import inject_principles
```

Then modify the chat function (around line 437, after transform_messages_for_openai call):

```python
    # Transform Anthropic-style messages to OpenAI-style for LiteLLM
    transformed_messages = transform_messages_for_openai(messages)

    # Inject conversation philosophy context
    user_id = data.get("user_id")
    transformed_messages = inject_principles(
        transformed_messages,
        str(user_id) if user_id else None
    )

    try:
        result = llm_chat(model=model, messages=transformed_messages, tools=tools)
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd src/mcp && python -m pytest tests/test_server_context.py -v`
Expected: PASS

- [ ] **Step 6: Run all existing tests**

Run: `cd src/mcp && python -m pytest tests/ -v`
Expected: All tests PASS

- [ ] **Step 7: Commit**

```bash
git add src/mcp/context.py src/mcp/server.py src/mcp/tests/test_server_context.py
git commit -m "feat: add conversation principles to chat context"
```

---

## Chunk 2: Philosophy File Loading

### Task 2: Load Philosophy File from Vault

**Files:**
- Modify: `src/mcp/context.py`
- Create: `src/mcp/tests/test_context.py`

- [ ] **Step 1: Write the failing test for philosophy loading**

```python
# src/mcp/tests/test_context.py
"""Tests for context loading module."""

import json
import os
import sys
import pytest
from pathlib import Path
from unittest.mock import patch

# Add parent directory to path for imports
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def test_load_philosophy_returns_file_content(tmp_path):
    """load_philosophy returns content from .lu/philosophy.md."""
    # Import after path setup
    import context

    # Create philosophy file
    lu_dir = tmp_path / ".lu"
    lu_dir.mkdir()
    philosophy_file = lu_dir / "philosophy.md"
    philosophy_file.write_text("# Custom Philosophy\n\nBe excellent.")

    with patch.object(context, "get_vault_path", return_value=tmp_path):
        result = context.load_philosophy()

    assert result is not None
    assert "Custom Philosophy" in result
    assert "Be excellent" in result


def test_load_philosophy_creates_default_if_missing(tmp_path):
    """load_philosophy creates default file if missing."""
    import context

    with patch.object(context, "get_vault_path", return_value=tmp_path):
        result = context.load_philosophy()

    # Should have created the file
    philosophy_file = tmp_path / ".lu" / "philosophy.md"
    assert philosophy_file.exists()

    # Should return default content
    assert "Scoping" in result
    assert "Pacing" in result
    assert "Ma" in result


def test_load_philosophy_handles_read_error(tmp_path):
    """load_philosophy returns None on read errors."""
    import context

    # Create a directory where the file should be (causes read error)
    lu_dir = tmp_path / ".lu"
    lu_dir.mkdir()
    (lu_dir / "philosophy.md").mkdir()  # Directory, not file

    with patch.object(context, "get_vault_path", return_value=tmp_path):
        result = context.load_philosophy()

    assert result is None
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_context.py::test_load_philosophy_returns_file_content -v`
Expected: FAIL - load_philosophy not defined

- [ ] **Step 3: Implement load_philosophy**

Add to `src/mcp/context.py` (after CORE_PRINCIPLES, before inject_principles):

```python
from pathlib import Path
from security import get_vault_path

DEFAULT_PHILOSOPHY = """# Conversation Philosophy

## Scoping

When you detect 2+ topics in a message:
1. Call conversation_scope to register them
2. Address the first naturally
3. After resolving, transition: "Now about [next topic]..."
4. If user redirects, follow their lead

## Pacing

- One question per message
- Acknowledge before asking next
- Don't stack questions

## Ma

Read the room:
- User finished something big → pause, appreciate
- User is venting → listen, don't solve immediately
- User is task-focused → stay efficient
- Silence is okay

## Anti-patterns

Avoid:
- Question dumps
- Rushing past emotional moments
- "Great! Awesome!" empty acknowledgments
- Forgetting topics that were raised
"""


def load_philosophy() -> str | None:
    """
    Load .lu/philosophy.md from vault, creating with defaults if missing.

    Returns:
        Philosophy content, or None on error
    """
    vault = get_vault_path()
    lu_dir = vault / ".lu"
    philosophy_file = lu_dir / "philosophy.md"

    try:
        if philosophy_file.exists():
            return philosophy_file.read_text()

        # Create default
        lu_dir.mkdir(parents=True, exist_ok=True)
        philosophy_file.write_text(DEFAULT_PHILOSOPHY)
        return DEFAULT_PHILOSOPHY

    except Exception:
        return None
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src/mcp && python -m pytest tests/test_context.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/mcp/context.py src/mcp/tests/test_context.py
git commit -m "feat: add philosophy file loading with defaults"
```

---

### Task 3: Include Philosophy in Chat Context

**Files:**
- Modify: `src/mcp/context.py`
- Modify: `src/mcp/tests/test_context.py`

- [ ] **Step 1: Write test for full context injection**

Add to `src/mcp/tests/test_context.py`:

```python
def test_inject_principles_includes_philosophy(tmp_path):
    """inject_principles includes philosophy file content."""
    import context

    # Create custom philosophy
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_context.py::test_inject_principles_includes_philosophy -v`
Expected: FAIL - philosophy content not included

- [ ] **Step 3: Update inject_principles to load philosophy**

Replace `inject_principles` in `src/mcp/context.py` with:

```python
def inject_principles(messages: list, user_id: str | None = None) -> list:
    """
    Inject conversation principles and philosophy into the system message.

    Includes:
    - Core principles (always)
    - Philosophy file content (if available)

    Args:
        messages: Original message list
        user_id: Optional user ID for loading topics (used in next task)

    Returns a new list (does not mutate input).
    """
    # Load philosophy (may be None)
    philosophy = load_philosophy()

    # Build full context
    context_parts = [CORE_PRINCIPLES.strip()]
    if philosophy:
        context_parts.append(f"\n## Philosophy Context\n\n{philosophy}")

    full_context = "\n".join(context_parts)

    result = []
    found_system = False

    for msg in messages:
        if msg.get("role") == "system" and not found_system:
            found_system = True
            result.append({
                "role": "system",
                "content": msg.get("content", "") + "\n\n" + full_context
            })
        else:
            result.append(msg)

    if not found_system:
        result.insert(0, {"role": "system", "content": full_context})

    return result
```

- [ ] **Step 4: Run all context tests**

Run: `cd src/mcp && python -m pytest tests/test_context.py -v`
Expected: All PASS

- [ ] **Step 5: Run all tests**

Run: `cd src/mcp && python -m pytest tests/ -v`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add src/mcp/context.py src/mcp/tests/test_context.py
git commit -m "feat: include philosophy file in chat context"
```

---

## Chunk 3: Open Topics in Context

### Task 4: Add Topics Context Loading

**Files:**
- Modify: `src/mcp/context.py`
- Modify: `src/mcp/tests/test_context.py`

- [ ] **Step 1: Write test for topics loading**

Add to `src/mcp/tests/test_context.py`:

```python
def test_load_topics_returns_open_topics(tmp_path):
    """load_topics returns open topics for a user."""
    import context

    # Create conversation state
    conv_dir = tmp_path / ".lu" / "conversations"
    conv_dir.mkdir(parents=True)
    state_file = conv_dir / "user_123.json"
    state_file.write_text(json.dumps({
        "id": "user_123",
        "topics": ["Project notes", "Recipe question"],
        "resolved": ["Birthday reminder"],
        "current": "Project notes"
    }))

    with patch.object(context, "get_vault_path", return_value=tmp_path):
        result = context.load_topics("user_123")

    assert result is not None
    assert "Project notes" in result
    assert "Recipe question" in result
    assert "current" in result.lower() or "Current" in result


def test_load_topics_returns_empty_for_no_state(tmp_path):
    """load_topics returns empty string when no state exists."""
    import context

    with patch.object(context, "get_vault_path", return_value=tmp_path):
        result = context.load_topics("nonexistent")

    assert result == ""
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_context.py::test_load_topics_returns_open_topics -v`
Expected: FAIL - load_topics not defined

- [ ] **Step 3: Implement load_topics**

Add `import json` at the top of `src/mcp/context.py` (with other imports), then add this function after load_philosophy:

```python
def load_topics(user_id: str) -> str:
    """
    Load open topics for a user from conversation state.

    Args:
        user_id: User identifier (e.g., Telegram user ID)

    Returns:
        Formatted string of open topics, or empty string if none
    """
    vault = get_vault_path()
    state_file = vault / ".lu" / "conversations" / f"{user_id}.json"

    if not state_file.exists():
        return ""

    try:
        state = json.loads(state_file.read_text())
        topics = state.get("topics", [])
        current = state.get("current")

        if not topics:
            return ""

        lines = []
        if current:
            lines.append(f"Current focus: {current}")
        lines.append(f"Open topics: {', '.join(topics)}")

        return "\n".join(lines)

    except Exception:
        return ""
```

- [ ] **Step 4: Run tests**

Run: `cd src/mcp && python -m pytest tests/test_context.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/mcp/context.py src/mcp/tests/test_context.py
git commit -m "feat: add topic loading for conversation context"
```

---

### Task 5: Include Topics in Chat Context

**Files:**
- Modify: `src/mcp/context.py`
- Modify: `src/mcp/tests/test_server_context.py`

- [ ] **Step 1: Write test for topics in chat**

Add to `src/mcp/tests/test_server_context.py`:

```python
def test_chat_includes_open_topics(app_client):
    """Chat endpoint should include open topics when user_id provided."""
    import context

    client, tmp_path = app_client

    # Create conversation state in mock vault
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
```

Also add `import json` at top of the test file (with other imports).

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_server_context.py::test_chat_includes_open_topics -v`
Expected: FAIL - topics not in context

- [ ] **Step 3: Update inject_principles to include topics**

Update `inject_principles` in `src/mcp/context.py`:

```python
def inject_principles(messages: list, user_id: str | None = None) -> list:
    """
    Inject conversation principles, philosophy, and topics into context.

    Args:
        messages: Original message list
        user_id: Optional user ID for loading topics

    Returns a new list (does not mutate input).
    """
    # Load philosophy
    philosophy = load_philosophy()

    # Load topics if user_id provided
    topics = load_topics(user_id) if user_id else ""

    # Build full context
    context_parts = [CORE_PRINCIPLES.strip()]
    if philosophy:
        context_parts.append(f"\n## Philosophy Context\n\n{philosophy}")
    if topics:
        context_parts.append(f"\n## Open Topics\n\n{topics}")

    full_context = "\n".join(context_parts)

    result = []
    found_system = False

    for msg in messages:
        if msg.get("role") == "system" and not found_system:
            found_system = True
            result.append({
                "role": "system",
                "content": msg.get("content", "") + "\n\n" + full_context
            })
        else:
            result.append(msg)

    if not found_system:
        result.insert(0, {"role": "system", "content": full_context})

    return result
```

- [ ] **Step 4: Run tests**

Run: `cd src/mcp && python -m pytest tests/test_server_context.py -v`
Expected: All PASS

- [ ] **Step 5: Run all tests**

Run: `cd src/mcp && python -m pytest tests/ -v`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add src/mcp/context.py src/mcp/tests/test_server_context.py
git commit -m "feat: include open topics in chat context"
```

---

## Chunk 4: Stale Topic Expiration

### Task 6: Add Topic Expiration Helper

**Files:**
- Modify: `src/mcp/tools/conversation.py`
- Create: `src/mcp/tests/test_conversation.py`

- [ ] **Step 1: Write test for stale topic expiration**

```python
# src/mcp/tests/test_conversation.py
"""Tests for conversation scope tool."""

import json
import os
import sys
import pytest
from datetime import datetime, timezone, timedelta
from pathlib import Path
from unittest.mock import patch

# Add parent directory to path for imports
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def test_expire_stale_topics_moves_old_topics(tmp_path):
    """expire_stale_topics moves topics older than max_age to stale."""
    from tools import conversation

    conv_dir = tmp_path / ".lu" / "conversations"
    conv_dir.mkdir(parents=True)

    # Create state with old updated timestamp
    old_time = (datetime.now(timezone.utc) - timedelta(hours=25)).isoformat()
    state_file = conv_dir / "user_123.json"
    state_file.write_text(json.dumps({
        "id": "user_123",
        "updated": old_time,
        "topics": ["Old topic"],
        "resolved": [],
        "current": "Old topic"
    }))

    with patch.object(conversation, "get_vault_path", return_value=tmp_path):
        count = conversation.expire_stale_topics("user_123", max_age_hours=24)

    assert count == 1

    # Verify state updated
    new_state = json.loads(state_file.read_text())
    assert "Old topic" not in new_state["topics"]
    assert "Old topic" in new_state.get("stale", [])


def test_expire_stale_topics_keeps_recent_topics(tmp_path):
    """expire_stale_topics keeps topics updated recently."""
    from tools import conversation

    conv_dir = tmp_path / ".lu" / "conversations"
    conv_dir.mkdir(parents=True)

    # Create state with recent timestamp
    recent_time = datetime.now(timezone.utc).isoformat()
    state_file = conv_dir / "user_123.json"
    state_file.write_text(json.dumps({
        "id": "user_123",
        "updated": recent_time,
        "topics": ["Recent topic"],
        "resolved": [],
        "current": "Recent topic"
    }))

    with patch.object(conversation, "get_vault_path", return_value=tmp_path):
        count = conversation.expire_stale_topics("user_123", max_age_hours=24)

    assert count == 0

    # Topics unchanged
    new_state = json.loads(state_file.read_text())
    assert "Recent topic" in new_state["topics"]


def test_expire_stale_topics_handles_missing_file(tmp_path):
    """expire_stale_topics returns 0 for missing conversation file."""
    from tools import conversation

    with patch.object(conversation, "get_vault_path", return_value=tmp_path):
        count = conversation.expire_stale_topics("nonexistent")

    assert count == 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_conversation.py::test_expire_stale_topics_moves_old_topics -v`
Expected: FAIL - expire_stale_topics not defined

- [ ] **Step 3: Implement expire_stale_topics**

Add to `src/mcp/tools/conversation.py` (after existing imports, add timedelta):

```python
from datetime import timedelta
```

Then add after the `_save_state` function:

```python
def expire_stale_topics(conversation_id: str, max_age_hours: int = 24) -> int:
    """
    Move topics older than max_age_hours to 'stale' status.

    Args:
        conversation_id: Conversation identifier
        max_age_hours: Hours after which topics become stale (default 24)

    Returns:
        Number of topics moved to stale
    """
    path = _get_state_path(conversation_id)
    if not path.exists():
        return 0

    try:
        state = json.loads(path.read_text())
    except Exception:
        return 0

    updated_str = state.get("updated")
    if not updated_str:
        return 0

    try:
        updated = datetime.fromisoformat(updated_str.replace("Z", "+00:00"))
    except Exception:
        return 0

    cutoff = datetime.now(timezone.utc) - timedelta(hours=max_age_hours)

    if updated >= cutoff:
        return 0

    # Move all topics to stale
    topics = state.get("topics", [])
    if not topics:
        return 0

    stale = state.get("stale", [])
    stale.extend(topics)

    state["stale"] = stale
    state["topics"] = []
    state["current"] = None
    state["updated"] = datetime.now(timezone.utc).isoformat()

    path.write_text(json.dumps(state, indent=2))

    return len(topics)
```

- [ ] **Step 4: Run tests**

Run: `cd src/mcp && python -m pytest tests/test_conversation.py -v`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/mcp/tools/conversation.py src/mcp/tests/test_conversation.py
git commit -m "feat: add stale topic expiration helper"
```

---

## Chunk 5: Integration Testing

### Task 7: End-to-End Integration Test

**Files:**
- Create: `src/mcp/tests/test_integration_philosophy.py`

- [ ] **Step 1: Write integration test**

```python
# src/mcp/tests/test_integration_philosophy.py
"""Integration tests for conversation philosophy system."""

import json
import os
import sys
import pytest
from pathlib import Path
from unittest.mock import patch

# Add parent directory to path for imports
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


@pytest.fixture
def vault_with_philosophy(tmp_path):
    """Create a vault with philosophy file and conversation state."""
    lu_dir = tmp_path / ".lu"
    lu_dir.mkdir()

    # Custom philosophy
    (lu_dir / "philosophy.md").write_text("# My Style\nBe excellent.")

    # User topics
    conv_dir = lu_dir / "conversations"
    conv_dir.mkdir()
    (conv_dir / "456.json").write_text(json.dumps({
        "id": "456",
        "topics": ["Build feature", "Fix bug"],
        "current": "Build feature",
        "updated": "2026-03-16T12:00:00Z"
    }))

    return tmp_path


def test_full_philosophy_context_flow(vault_with_philosophy):
    """
    Integration test: Full flow from request to LLM call.

    Verifies:
    1. Core principles injected
    2. Philosophy file loaded
    3. Topics included for user
    """
    import context
    from server import app

    # Set required env vars
    os.environ.setdefault("AUTH_TOKEN", "test-token")
    os.environ.setdefault("VAULT_PATH", str(vault_with_philosophy))

    app.config["TESTING"] = True
    client = app.test_client()

    captured_messages = None

    def capture_llm_call(**kwargs):
        nonlocal captured_messages
        captured_messages = kwargs.get("messages", [])
        return {"content": "Got it!", "tool_calls": None, "usage": {}}

    with patch("server.llm_chat", side_effect=capture_llm_call), \
         patch("security.get_vault_path", return_value=vault_with_philosophy), \
         patch("security._vault_path", vault_with_philosophy), \
         patch("security._auth_token", "test-token"), \
         patch.object(context, "get_vault_path", return_value=vault_with_philosophy):

        response = client.post(
            "/chat",
            json={
                "messages": [
                    {"role": "system", "content": "You are Lu."},
                    {"role": "user", "content": "What's on my list?"}
                ],
                "user_id": 456
            },
            headers={"Authorization": "Bearer test-token"}
        )

    assert response.status_code == 200
    assert captured_messages is not None

    # Find system message
    system_msg = next(
        (m for m in captured_messages if m.get("role") == "system"),
        None
    )
    assert system_msg is not None

    content = system_msg["content"]

    # Check all components present
    assert "CONVERSATION PRINCIPLES" in content, "Core principles missing"
    assert "Scoping" in content
    assert "Pacing" in content
    assert "Ma" in content
    assert "My Style" in content, "Philosophy file content missing"
    assert "Be excellent" in content
    assert "Build feature" in content, "Topics missing"
    assert "Fix bug" in content
```

- [ ] **Step 2: Run integration test**

Run: `cd src/mcp && python -m pytest tests/test_integration_philosophy.py -v`
Expected: PASS

- [ ] **Step 3: Run full test suite**

Run: `cd src/mcp && python -m pytest tests/ -v`
Expected: All PASS

- [ ] **Step 4: Commit**

```bash
git add src/mcp/tests/test_integration_philosophy.py
git commit -m "test: add integration test for philosophy context"
```

---

## Chunk 6: Final Verification

### Task 8: Final Cleanup and Verification

- [ ] **Step 1: Run full test suite**

Run: `cd src/mcp && python -m pytest tests/ -v --tb=short`
Expected: All PASS

- [ ] **Step 2: Review all changes**

```bash
git log --oneline -10  # Review commits
git diff develop  # Review all changes from branch start
```

- [ ] **Step 3: Create summary commit message**

```bash
git log --oneline --no-walk HEAD~6..HEAD
```

Expected commits:
1. feat: add conversation principles to chat context
2. feat: add philosophy file loading with defaults
3. feat: include philosophy file in chat context
4. feat: add topic loading for conversation context
5. feat: include open topics in chat context
6. feat: add stale topic expiration helper
7. test: add integration test for philosophy context

---

## Verification Checklist

- [ ] All tests pass: `cd src/mcp && python -m pytest tests/ -v`
- [ ] Chat endpoint injects conversation principles
- [ ] Philosophy file loads from vault (or creates default)
- [ ] Open topics appear in context when user_id provided
- [ ] Stale topic expiration works
- [ ] No regressions in existing functionality
