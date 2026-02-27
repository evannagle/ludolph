# LLM Proxy Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Route all LLM requests through the MCP server using LiteLLM, enabling multi-provider support.

**Architecture:** Pi client calls MCP server's new `/chat` endpoint instead of Anthropic directly. MCP server uses LiteLLM library to route to Claude, GPT, Ollama, etc. Provider auth is centralized on Mac.

**Tech Stack:** Python (Flask + LiteLLM), Rust (reqwest for HTTP)

---

## Phase 1: MCP Server - Add LLM Proxy

### Task 1: Add LiteLLM Dependency

**Files:**
- Modify: `src/mcp/pyproject.toml`

**Step 1: Add litellm to dependencies**

Edit `src/mcp/pyproject.toml`:

```toml
[project]
name = "ludolph-mcp"
version = "0.5.0"
description = "MCP server for Ludolph - general-purpose filesystem access"
requires-python = ">=3.11"
dependencies = [
    "flask>=2.0",
    "litellm>=1.0",
]
```

**Step 2: Install dependencies**

Run: `cd src/mcp && pip install -e .`
Expected: Successfully installed litellm and dependencies

**Step 3: Verify import works**

Run: `python -c "import litellm; print(litellm.__version__)"`
Expected: Version number printed (e.g., "1.56.4")

**Step 4: Commit**

```bash
git add src/mcp/pyproject.toml
git commit -m "feat(mcp): add litellm dependency for multi-provider LLM support"
```

---

### Task 2: Create LLM Module

**Files:**
- Create: `src/mcp/llm.py`
- Test: `src/mcp/tests/test_llm.py`

**Step 1: Write the failing test**

Create `src/mcp/tests/test_llm.py`:

```python
"""Tests for LLM proxy module."""

import pytest
from unittest.mock import patch, MagicMock


def test_chat_returns_response():
    """Chat endpoint returns content from LiteLLM."""
    from mcp.llm import chat

    mock_response = MagicMock()
    mock_response.choices = [MagicMock()]
    mock_response.choices[0].message.content = "Hello!"
    mock_response.choices[0].message.tool_calls = None
    mock_response.usage = MagicMock()
    mock_response.usage._asdict = lambda: {"prompt_tokens": 10, "completion_tokens": 5}

    with patch("mcp.llm.completion", return_value=mock_response):
        result = chat(
            model="claude-sonnet-4",
            messages=[{"role": "user", "content": "Hi"}],
        )

    assert result["content"] == "Hello!"
    assert result["tool_calls"] is None
    assert "usage" in result


def test_chat_handles_tool_calls():
    """Chat returns tool_calls when present."""
    from mcp.llm import chat

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

    with patch("mcp.llm.completion", return_value=mock_response):
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
    from mcp.llm import chat, LlmAuthError
    import litellm

    with patch("mcp.llm.completion", side_effect=litellm.AuthenticationError(
        message="Invalid API key",
        llm_provider="anthropic",
        model="claude-sonnet-4",
    )):
        with pytest.raises(LlmAuthError):
            chat(model="claude-sonnet-4", messages=[{"role": "user", "content": "Hi"}])


def test_chat_raises_on_budget_exceeded():
    """Chat raises appropriate error when budget is exceeded."""
    from mcp.llm import chat, LlmBudgetError
    import litellm

    with patch("mcp.llm.completion", side_effect=litellm.BudgetExceededError(
        message="Budget exceeded",
        current_cost=100.0,
        max_budget=50.0,
    )):
        with pytest.raises(LlmBudgetError):
            chat(model="claude-sonnet-4", messages=[{"role": "user", "content": "Hi"}])
```

**Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_llm.py -v`
Expected: FAIL with "ModuleNotFoundError: No module named 'mcp.llm'"

**Step 3: Write minimal implementation**

Create `src/mcp/llm.py`:

```python
"""LLM proxy module using LiteLLM for multi-provider support."""

from typing import Any

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
```

**Step 4: Run test to verify it passes**

Run: `cd src/mcp && python -m pytest tests/test_llm.py -v`
Expected: All tests PASS

**Step 5: Commit**

```bash
git add src/mcp/llm.py src/mcp/tests/test_llm.py
git commit -m "feat(mcp): add LLM module with LiteLLM integration"
```

---

### Task 3: Add /chat Endpoint to Server

**Files:**
- Modify: `src/mcp/server.py`
- Test: `src/mcp/tests/test_server_chat.py`

**Step 1: Write the failing test**

Create `src/mcp/tests/test_server_chat.py`:

```python
"""Tests for /chat endpoint."""

import pytest
from unittest.mock import patch


@pytest.fixture
def client():
    """Create test client with auth configured."""
    import os
    os.environ["VAULT_PATH"] = "/tmp/test-vault"
    os.environ["AUTH_TOKEN"] = "test-token"

    from mcp.server import app
    from mcp.security import init_security
    from pathlib import Path

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
```

**Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_server_chat.py -v`
Expected: FAIL with "404 NOT FOUND" (endpoint doesn't exist)

**Step 3: Add /chat endpoint to server**

Modify `src/mcp/server.py`, add after line 68 (after tools_call function):

```python
from .llm import chat as llm_chat, LlmAuthError, LlmBudgetError, LlmRateLimitError, LlmApiError


@app.route("/chat", methods=["POST"])
@require_auth
def chat():
    """Proxy chat request to LLM provider via LiteLLM."""
    data = request.json or {}
    model = data.get("model", "claude-sonnet-4")
    messages = data.get("messages", [])
    tools = data.get("tools")

    try:
        result = llm_chat(model=model, messages=messages, tools=tools)
        return jsonify(result)
    except LlmAuthError as e:
        return jsonify({"error": "auth_failed", "message": str(e)}), 401
    except LlmBudgetError as e:
        return jsonify({"error": "budget_exceeded", "message": str(e)}), 402
    except LlmRateLimitError as e:
        return jsonify({"error": "rate_limit", "message": str(e)}), 429
    except LlmApiError as e:
        return jsonify({"error": "api_error", "message": str(e)}), 502
```

**Step 4: Run test to verify it passes**

Run: `cd src/mcp && python -m pytest tests/test_server_chat.py -v`
Expected: All tests PASS

**Step 5: Commit**

```bash
git add src/mcp/server.py src/mcp/tests/test_server_chat.py
git commit -m "feat(mcp): add /chat endpoint for LLM proxy"
```

---

### Task 4: Add /chat/stream Endpoint (SSE)

**Files:**
- Modify: `src/mcp/server.py`
- Modify: `src/mcp/llm.py`
- Test: `src/mcp/tests/test_llm.py` (add streaming test)

**Step 1: Write the failing test**

Add to `src/mcp/tests/test_llm.py`:

```python
def test_chat_stream_yields_chunks():
    """Streaming chat yields content chunks."""
    from mcp.llm import chat_stream

    mock_chunk1 = MagicMock()
    mock_chunk1.choices = [MagicMock()]
    mock_chunk1.choices[0].delta.content = "Hello"
    mock_chunk1.choices[0].delta.tool_calls = None

    mock_chunk2 = MagicMock()
    mock_chunk2.choices = [MagicMock()]
    mock_chunk2.choices[0].delta.content = " world!"
    mock_chunk2.choices[0].delta.tool_calls = None

    with patch("mcp.llm.completion", return_value=iter([mock_chunk1, mock_chunk2])):
        chunks = list(chat_stream(
            model="claude-sonnet-4",
            messages=[{"role": "user", "content": "Hi"}],
        ))

    assert len(chunks) == 2
    assert chunks[0]["content"] == "Hello"
    assert chunks[1]["content"] == " world!"
```

**Step 2: Run test to verify it fails**

Run: `cd src/mcp && python -m pytest tests/test_llm.py::test_chat_stream_yields_chunks -v`
Expected: FAIL with "cannot import name 'chat_stream'"

**Step 3: Add streaming to llm.py**

Add to `src/mcp/llm.py`:

```python
from typing import Iterator


def chat_stream(
    model: str,
    messages: list[dict[str, Any]],
    tools: list[dict[str, Any]] | None = None,
) -> Iterator[dict[str, Any]]:
    """
    Stream a chat request, yielding chunks as they arrive.

    Yields:
        Dict with "content" and/or "tool_calls" for each chunk
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
                "tool_calls": None,  # Tool calls handled in final message
            }

    except litellm.AuthenticationError as e:
        raise LlmAuthError(str(e)) from e
    except litellm.BudgetExceededError as e:
        raise LlmBudgetError(str(e)) from e
    except litellm.RateLimitError as e:
        raise LlmRateLimitError(str(e)) from e
    except litellm.APIError as e:
        raise LlmApiError(str(e)) from e
```

**Step 4: Add /chat/stream endpoint to server**

Add to `src/mcp/server.py`:

```python
from flask import Response
from .llm import chat_stream as llm_chat_stream
import json


@app.route("/chat/stream", methods=["POST"])
@require_auth
def chat_stream():
    """Stream chat response via Server-Sent Events."""
    data = request.json or {}
    model = data.get("model", "claude-sonnet-4")
    messages = data.get("messages", [])
    tools = data.get("tools")

    def generate():
        try:
            for chunk in llm_chat_stream(model=model, messages=messages, tools=tools):
                yield f"data: {json.dumps(chunk)}\n\n"
            yield "data: [DONE]\n\n"
        except LlmAuthError as e:
            yield f"data: {json.dumps({'error': 'auth_failed', 'message': str(e)})}\n\n"
        except LlmBudgetError as e:
            yield f"data: {json.dumps({'error': 'budget_exceeded', 'message': str(e)})}\n\n"
        except LlmRateLimitError as e:
            yield f"data: {json.dumps({'error': 'rate_limit', 'message': str(e)})}\n\n"
        except LlmApiError as e:
            yield f"data: {json.dumps({'error': 'api_error', 'message': str(e)})}\n\n"

    return Response(generate(), mimetype="text/event-stream")
```

**Step 5: Run tests**

Run: `cd src/mcp && python -m pytest tests/test_llm.py -v`
Expected: All tests PASS

**Step 6: Commit**

```bash
git add src/mcp/server.py src/mcp/llm.py src/mcp/tests/test_llm.py
git commit -m "feat(mcp): add /chat/stream endpoint with SSE support"
```

---

## Phase 2: Rust Client - Replace Claude with LLM Client

### Task 5: Update Config Schema

**Files:**
- Modify: `src/config.rs`

**Step 1: Add LlmConfig alongside ClaudeConfig (backward compatible)**

Modify `src/config.rs`:

```rust
/// LLM configuration (new style - provider-agnostic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default = "default_model")]
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
        }
    }
}
```

Add to Config struct (after `claude` field):

```rust
    /// LLM configuration (new style - uses MCP proxy)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmConfig>,
```

**Step 2: Build to verify**

Run: `cargo build`
Expected: Compiles without errors

**Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): add LlmConfig for provider-agnostic model selection"
```

---

### Task 6: Add Chat Methods to McpClient

**Files:**
- Modify: `src/mcp_client.rs`

**Step 1: Add ChatRequest and ChatResponse types**

Add to `src/mcp_client.rs`:

```rust
#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<serde_json::Value>>,
}

#[derive(Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: ChatContent,
}

#[derive(Serialize, Clone)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Blocks(Vec<serde_json::Value>),
}

#[derive(Deserialize)]
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default)]
    pub usage: serde_json::Value,
}

#[derive(Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Deserialize, Clone)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Deserialize)]
pub struct ChatError {
    pub error: String,
    pub message: String,
}
```

**Step 2: Add chat method to McpClient**

Add to `impl McpClient`:

```rust
    /// Send a chat request to the MCP server's LLM proxy.
    pub async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse> {
        let response = self
            .client
            .post(format!("{}/chat", self.base_url))
            .header("Authorization", format!("Bearer {}", self.auth_token))
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await
            .map_err(|e| Self::format_connection_error(&e, &self.base_url, "chat"))?;

        let status = response.status();

        if !status.is_success() {
            let error: ChatError = response.json().await.unwrap_or(ChatError {
                error: "unknown".to_string(),
                message: format!("HTTP {}", status),
            });

            let msg = match error.error.as_str() {
                "auth_failed" => "Invalid API credentials. Check MCP server config.",
                "budget_exceeded" => "Credits exhausted. Add credits or switch models.",
                "rate_limit" => "Rate limited. Wait and retry.",
                _ => &error.message,
            };

            return Err(anyhow::anyhow!(msg.to_string()));
        }

        response.json().await.context("Failed to parse chat response")
    }
```

**Step 3: Build to verify**

Run: `cargo build`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add src/mcp_client.rs
git commit -m "feat(mcp_client): add chat method for LLM proxy"
```

---

### Task 7: Create LLM Module

**Files:**
- Create: `src/llm.rs`
- Modify: `src/main.rs`

**Step 1: Create src/llm.rs**

```rust
//! LLM client that proxies through MCP server.
//!
//! Replaces direct Anthropic API calls with MCP-proxied requests,
//! enabling multi-provider support via LiteLLM on the server.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::config::Config;
use crate::mcp_client::{ChatContent, ChatMessage, ChatRequest, ChatResponse, McpClient, ToolCall};
use crate::memory::Memory;
use crate::setup::SETUP_COMPLETE_MARKER;
use crate::tools::{Tool, execute_tool_local};

/// Result of a setup-aware chat session.
pub struct SetupChatResult {
    /// The response text from the LLM.
    pub response: String,
    /// Whether `complete_setup` was called during the conversation.
    pub setup_completed: bool,
}

/// Tool execution backend.
#[derive(Clone)]
enum ToolBackend {
    /// Local filesystem access (Mac or standalone Pi with local vault)
    Local { vault_path: std::path::PathBuf },
    /// Remote MCP server (Pi thin client connecting to Mac)
    Mcp { client: McpClient },
}

/// LLM client with tool execution support.
pub struct Llm {
    mcp_client: McpClient,
    model: String,
    tool_backend: ToolBackend,
    memory: Option<Arc<Memory>>,
}

impl Clone for Llm {
    fn clone(&self) -> Self {
        Self {
            mcp_client: self.mcp_client.clone(),
            model: self.model.clone(),
            tool_backend: self.tool_backend.clone(),
            memory: self.memory.clone(),
        }
    }
}

impl Llm {
    /// Create an LLM client from config with optional memory.
    pub fn from_config_with_memory(config: &Config, memory: Option<Arc<Memory>>) -> Result<Self> {
        let mcp_config = config.mcp.as_ref()
            .context("MCP configuration required for LLM proxy")?;

        let mcp_client = McpClient::from_config(mcp_config);

        let tool_backend = if config.vault.is_some() {
            ToolBackend::Local {
                vault_path: config.vault.as_ref().unwrap().path.clone(),
            }
        } else {
            ToolBackend::Mcp {
                client: mcp_client.clone(),
            }
        };

        // Get model from [llm] section, fall back to [claude] for backward compatibility
        let model = config.llm.as_ref()
            .map(|l| l.model.clone())
            .or_else(|| config.claude.as_ref().map(|c| c.model.clone()))
            .unwrap_or_else(|| "claude-sonnet-4".to_string());

        Ok(Self {
            mcp_client,
            model,
            tool_backend,
            memory,
        })
    }

    /// Get the vault path description for the system prompt.
    fn vault_description(&self) -> String {
        match &self.tool_backend {
            ToolBackend::Local { vault_path } => vault_path.display().to_string(),
            ToolBackend::Mcp { .. } => "your Mac (via MCP)".to_string(),
        }
    }

    /// Execute a tool using the configured backend.
    async fn execute_tool(&self, name: &str, input: &Value) -> String {
        match &self.tool_backend {
            ToolBackend::Local { vault_path } => execute_tool_local(name, input, vault_path).await,
            ToolBackend::Mcp { client } => client
                .call_tool(name, input)
                .await
                .unwrap_or_else(|e| format!("Error: {e}")),
        }
    }

    /// Get tool definitions from the configured backend.
    async fn get_tools(&self) -> Result<Vec<Tool>> {
        match &self.tool_backend {
            ToolBackend::Local { .. } => Ok(crate::tools::get_tool_definitions()),
            ToolBackend::Mcp { client } => client.get_tool_definitions().await,
        }
    }

    /// Build system prompt with memory and vault context.
    async fn build_system_prompt(&self) -> String {
        let memory_context = if self.memory.is_some() {
            "\n\nYou have access to conversation history with this user."
        } else {
            ""
        };

        let lu_context = self
            .load_lu_context()
            .await
            .map_or_else(String::new, |content| {
                format!("\n\n## Vault Context (from Lu.md)\n\n{content}")
            });

        format!(
            "You are Ludolph, a helpful assistant with access to the user's Obsidian vault at {}. \
             Be concise and helpful.{}{}",
            self.vault_description(),
            memory_context,
            lu_context
        )
    }

    /// Load conversation history from memory.
    fn load_conversation_history(&self, user_id: Option<i64>) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let context = memory.get_context(uid).unwrap_or_default();
            for msg in context {
                messages.push(ChatMessage {
                    role: msg.role.clone(),
                    content: ChatContent::Text(msg.content.clone()),
                });
            }
        }

        messages
    }

    /// Store message in memory.
    fn store_message(&self, user_id: Option<i64>, role: &str, content: &str) {
        if let (Some(memory), Some(uid)) = (&self.memory, user_id) {
            let _ = memory.add_message(uid, role, content);
        }
    }

    /// Try to load Lu.md content from the vault.
    async fn load_lu_context(&self) -> Option<String> {
        let result = self
            .execute_tool("read_file", &serde_json::json!({"path": "Lu.md"}))
            .await;

        if result.contains("Error:") || result.contains("not found") || result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Convert tools to JSON format for API.
    fn tools_to_json(tools: &[Tool]) -> Vec<Value> {
        tools.iter().map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            })
        }).collect()
    }

    /// Process tool calls and return results.
    async fn process_tool_calls(&self, tool_calls: &[ToolCall]) -> Vec<Value> {
        let mut results = Vec::new();

        for tc in tool_calls {
            let input: Value = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(Value::Object(Default::default()));

            let result = self.execute_tool(&tc.function.name, &input).await;

            results.push(serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tc.id,
                "content": result,
            }));
        }

        results
    }

    /// Chat with the LLM, handling tool calls.
    pub async fn chat(&self, user_message: &str, user_id: Option<i64>) -> Result<String> {
        let tools = self.get_tools().await?;
        let system = self.build_system_prompt().await;

        let mut messages = self.load_conversation_history(user_id);
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Text(user_message.to_string()),
        });

        // Store user message
        self.store_message(user_id, "user", user_message);

        // Add system message at start
        messages.insert(0, ChatMessage {
            role: "system".to_string(),
            content: ChatContent::Text(system),
        });

        // Tool loop
        loop {
            let request = ChatRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: Some(Self::tools_to_json(&tools)),
            };

            let response = self.mcp_client.chat(&request).await?;

            if let Some(tool_calls) = &response.tool_calls {
                if !tool_calls.is_empty() {
                    // Add assistant message with tool calls
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: ChatContent::Blocks(vec![serde_json::json!({
                            "type": "tool_use",
                            "tool_calls": tool_calls,
                        })]),
                    });

                    // Execute tools and add results
                    let results = self.process_tool_calls(tool_calls).await;
                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: ChatContent::Blocks(results),
                    });

                    continue;
                }
            }

            // No tool calls, return content
            let content = response.content.unwrap_or_default();
            self.store_message(user_id, "assistant", &content);
            return Ok(content);
        }
    }

    /// Chat with streaming support.
    pub async fn chat_streaming<F>(
        &self,
        user_message: &str,
        user_id: Option<i64>,
        on_text: F,
    ) -> Result<String>
    where
        F: Fn(&str) + Send + Sync,
    {
        // For now, fall back to non-streaming
        // TODO: Implement SSE client for streaming
        let result = self.chat(user_message, user_id).await?;
        on_text(&result);
        Ok(result)
    }

    /// Chat with custom system prompt (for setup mode).
    pub async fn chat_with_system(
        &self,
        user_message: &str,
        system_prompt: &str,
        user_id: Option<i64>,
    ) -> Result<SetupChatResult> {
        let tools = self.get_tools().await?;

        let mut messages = self.load_conversation_history(user_id);
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Text(user_message.to_string()),
        });

        self.store_message(user_id, "user", user_message);

        messages.insert(0, ChatMessage {
            role: "system".to_string(),
            content: ChatContent::Text(system_prompt.to_string()),
        });

        let mut setup_completed = false;

        loop {
            let request = ChatRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: Some(Self::tools_to_json(&tools)),
            };

            let response = self.mcp_client.chat(&request).await?;

            if let Some(tool_calls) = &response.tool_calls {
                if !tool_calls.is_empty() {
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: ChatContent::Blocks(vec![serde_json::json!({
                            "type": "tool_use",
                            "tool_calls": tool_calls,
                        })]),
                    });

                    let mut results = Vec::new();
                    for tc in tool_calls {
                        let input: Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(Value::Object(Default::default()));

                        let result = self.execute_tool(&tc.function.name, &input).await;

                        if tc.function.name == "complete_setup" && result.contains(SETUP_COMPLETE_MARKER) {
                            setup_completed = true;
                        }

                        results.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": tc.id,
                            "content": result,
                        }));
                    }

                    messages.push(ChatMessage {
                        role: "user".to_string(),
                        content: ChatContent::Blocks(results),
                    });

                    continue;
                }
            }

            let content = response.content.unwrap_or_default();
            self.store_message(user_id, "assistant", &content);

            return Ok(SetupChatResult {
                response: content,
                setup_completed,
            });
        }
    }
}
```

**Step 2: Add module to main.rs**

Add to `src/main.rs` after other mod declarations:

```rust
mod llm;
```

**Step 3: Build to verify**

Run: `cargo build`
Expected: Compiles (may have warnings about unused code)

**Step 4: Commit**

```bash
git add src/llm.rs src/main.rs
git commit -m "feat: add LLM module for MCP-proxied chat"
```

---

### Task 8: Update Bot to Use LLM Module

**Files:**
- Modify: `src/bot.rs`

**Step 1: Replace Claude import with Llm**

Change line 16 from:
```rust
use crate::claude::Claude;
```
to:
```rust
use crate::llm::Llm;
```

**Step 2: Update bot initialization**

Change line 207 from:
```rust
    let claude = Claude::from_config_with_memory(&config, memory);
```
to:
```rust
    let llm = Llm::from_config_with_memory(&config, memory)?;
```

**Step 3: Replace all `claude` with `llm` in the handler**

Use find/replace: `claude` → `llm` (case-sensitive, whole word)

**Step 4: Build to verify**

Run: `cargo build`
Expected: Compiles without errors

**Step 5: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 6: Commit**

```bash
git add src/bot.rs
git commit -m "refactor(bot): use Llm module instead of Claude"
```

---

### Task 9: Remove Anthropic SDK Dependency

**Files:**
- Modify: `Cargo.toml`
- Delete: `src/claude.rs`
- Modify: `src/main.rs`

**Step 1: Remove anthropic-sdk-rust from Cargo.toml**

Remove this line from dependencies:
```toml
anthropic-sdk-rust = "0.1"
```

**Step 2: Remove claude module from main.rs**

Remove:
```rust
mod claude;
```

**Step 3: Delete src/claude.rs**

Run: `rm src/claude.rs`

**Step 4: Build to verify**

Run: `cargo build`
Expected: Compiles without errors

**Step 5: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 6: Commit**

```bash
git add Cargo.toml src/main.rs
git rm src/claude.rs
git commit -m "refactor: remove anthropic-sdk-rust dependency, use MCP proxy"
```

---

## Phase 3: Integration Testing

### Task 10: End-to-End Test

**Step 1: Start MCP server with Claude API key**

```bash
cd src/mcp
VAULT_PATH=~/Vaults/Noggin/noggin \
AUTH_TOKEN=test-token \
ANTHROPIC_API_KEY=sk-ant-... \
python -m mcp.server
```

**Step 2: Test /chat endpoint directly**

```bash
curl -X POST http://localhost:8200/chat \
  -H "Authorization: Bearer test-token" \
  -H "Content-Type: application/json" \
  -d '{"model": "claude-sonnet-4", "messages": [{"role": "user", "content": "Hello!"}]}'
```

Expected: JSON response with "content" field

**Step 3: Update Pi config**

```toml
[llm]
model = "claude-sonnet-4"

[mcp]
url = "http://mac:8200"
auth_token = "test-token"
```

**Step 4: Run bot locally**

```bash
cargo run
```

**Step 5: Test via Telegram**

Send a message to the bot, verify response comes through.

**Step 6: Commit integration test docs**

```bash
git add docs/plans/2026-02-27-llm-proxy-implementation.md
git commit -m "docs: complete LLM proxy implementation plan"
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Add LiteLLM dependency | pyproject.toml |
| 2 | Create LLM module | llm.py, test_llm.py |
| 3 | Add /chat endpoint | server.py, test_server_chat.py |
| 4 | Add /chat/stream endpoint | server.py, llm.py |
| 5 | Update config schema | config.rs |
| 6 | Add chat to McpClient | mcp_client.rs |
| 7 | Create Llm module | llm.rs, main.rs |
| 8 | Update bot to use Llm | bot.rs |
| 9 | Remove Anthropic SDK | Cargo.toml, claude.rs |
| 10 | End-to-end testing | Manual verification |

**Total: 10 tasks, ~15 commits**
