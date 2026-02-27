# LLM Proxy Design: Multi-Provider Support via MCP Server

## Problem

Ludolph's Pi client calls the Anthropic API directly with a prepaid API key. This creates issues:
- Credits run out unexpectedly
- No way to use Claude Code Max subscription
- Locked to one provider (no GPT, Llama, Gemini)
- Each user needs their own API key on the Pi

## Solution

Embed LiteLLM in the existing MCP server on Mac. The Pi becomes a thin client that sends chat requests to the MCP server, which routes to any LLM provider.

## Architecture

```
┌─────────────────┐     HTTP      ┌─────────────────────────────┐
│   Pi (Rust)     │──────────────▶│   Mac MCP Server (Python)   │
│                 │               │                             │
│  Telegram Bot   │               │  /tools/call  (vault ops)   │
│  LlmClient      │               │  /chat        (LLM proxy)   │
│                 │               │                             │
└─────────────────┘               │  litellm library            │
                                  └──────────────┬──────────────┘
                                                 │
                         ┌───────────────────────┼───────────────────────┐
                         │                       │                       │
                         ▼                       ▼                       ▼
                   ┌──────────┐           ┌──────────┐           ┌──────────┐
                   │  Claude  │           │  OpenAI  │           │  Ollama  │
                   │  (Max)   │           │  (API)   │           │  (local) │
                   └──────────┘           └──────────┘           └──────────┘
```

## MCP Server Changes

### New Endpoints

**POST /chat**
```python
@app.route("/chat", methods=["POST"])
@require_auth
def chat():
    data = request.json
    response = completion(
        model=data["model"],
        messages=data["messages"],
        tools=data.get("tools"),
    )
    return jsonify({
        "content": response.choices[0].message.content,
        "tool_calls": response.choices[0].message.tool_calls,
        "usage": dict(response.usage),
    })
```

**POST /chat/stream** - SSE streaming for real-time responses

### Provider Configuration

Environment variables on Mac:
```bash
# Claude Code Max (OAuth)
LITELLM_CLAUDE_CODE_TOKEN="oauth-token"

# Claude API (prepaid)
ANTHROPIC_API_KEY="sk-ant-..."

# OpenAI
OPENAI_API_KEY="sk-..."

# Ollama - no key needed, just run ollama
```

### Dependencies

```
pip install litellm
```

## Rust Client Changes

### Replace claude.rs with llm.rs

```rust
pub struct LlmClient {
    mcp_client: McpClient,
    model: String,
}

impl LlmClient {
    pub async fn chat(&self, messages: Vec<Message>, tools: Vec<Tool>) -> Result<Response> {
        self.mcp_client.post("/chat", json!({
            "model": self.model,
            "messages": messages,
            "tools": tools,
        })).await
    }

    pub async fn chat_stream(&self, ..., on_text: impl Fn(&str)) -> Result<Response> {
        // SSE stream from /chat/stream
    }
}
```

### Config Changes

Before:
```toml
[claude]
api_key = "sk-ant-..."
model = "claude-sonnet-4"
```

After:
```toml
[llm]
model = "claude-sonnet-4"  # or "gpt-4o", "ollama/llama3"
```

### Removed Dependencies

- `anthropic-sdk-rust` crate no longer needed
- No API keys on Pi

## Error Handling

MCP server returns structured errors:

| Status | Error | Meaning |
|--------|-------|---------|
| 401 | auth_failed | Invalid API key or OAuth token |
| 402 | budget_exceeded | Credits exhausted |
| 429 | rate_limit | Rate limited |
| 502 | api_error | Provider error |

Rust client surfaces these with actionable messages.

## Memory/Logging

- MCP server is stateless (no logging of conversations)
- Pi continues to handle memory via existing SQLite + vault persistence
- Separation of concerns: proxy routes, Pi owns state

## Migration Path

**Phase 1: MCP Server**
- Add litellm dependency
- Add /chat and /chat/stream endpoints
- Configure Claude API provider
- Test end-to-end

**Phase 2: Pi Client**
- Replace claude.rs with llm.rs
- Remove anthropic-sdk-rust
- Update config schema
- Deploy to Pi

**Phase 3: Additional Providers**
- Configure Claude Code Max (OAuth)
- Add OpenAI, Ollama as needed
- Test model switching

## Files Affected

### MCP Server (Python)
- `src/mcp/server.py` - Add /chat endpoints
- `src/mcp/llm.py` - New: LiteLLM wrapper
- `requirements.txt` - Add litellm

### Pi Client (Rust)
- `src/claude.rs` → `src/llm.rs` - Replace with MCP-based client
- `src/config.rs` - [claude] → [llm]
- `src/bot.rs` - Use LlmClient
- `src/main.rs` - Update module
- `Cargo.toml` - Remove anthropic-sdk-rust

## Benefits

- Use Claude Code Max subscription (no separate API credits)
- Switch models via config, not code
- Support 100+ providers via LiteLLM
- Centralized auth on Mac
- Simpler Pi deployment (no API keys)
- Provider-agnostic bot
