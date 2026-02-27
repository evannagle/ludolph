# Phase 2: Core UX Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add streaming responses, thread support, and enhanced error reporting for responsive, engaging conversations.

**Architecture:** Extend `src/claude.rs` with streaming callback support, enhance `src/bot.rs` to edit messages in real-time, add thread preservation, and improve error formatting across tools.

**Tech Stack:** Rust, teloxide 0.13, anthropic-sdk-rust 0.1 (with streaming), tokio

---

### Task 1: Add Streaming Response Support

**Files:**
- Modify: `src/claude.rs:169-297` (chat method)
- Modify: `src/bot.rs` (message sending)

**Context:** The anthropic-sdk-rust crate supports streaming via `MessageStream` and `MessageStreamEvent`. We'll modify the chat method to accept an optional callback that receives text deltas, allowing bot.rs to edit the Telegram message in real-time.

**Step 1: Check anthropic-sdk streaming API**

Research the SDK documentation:
```bash
cargo doc --open --package anthropic-sdk-rust
```

Look for:
- How to create a streaming request
- `MessageStream` struct
- `MessageStreamEvent` enum
- How to extract text deltas

**Step 2: Add streaming callback to chat method signature**

In `src/claude.rs`, modify the `chat()` method signature (line 169):

```rust
pub async fn chat<F>(
    &self,
    user_message: &str,
    user_id: Option<i64>,
    on_delta: Option<F>,
) -> Result<String>
where
    F: Fn(&str) + Send + Sync,
```

The callback receives text deltas as they arrive from Claude.

**Step 3: Implement streaming in chat method**

Replace the `self.client.messages().create(params).await` call with streaming logic:

```rust
use anthropic_sdk::streaming::{MessageStream, MessageStreamEvent};

// Check if streaming requested
let response_text = if on_delta.is_some() {
    // Streaming path
    let mut stream = self.client.messages().create_stream(params).await?;
    let mut accumulated = String::new();

    while let Some(event) = stream.next().await {
        match event? {
            MessageStreamEvent::ContentBlockDelta { delta, .. } => {
                if let Some(text) = delta.text {
                    accumulated.push_str(&text);
                    if let Some(ref callback) = on_delta {
                        callback(&accumulated);
                    }
                }
            }
            MessageStreamEvent::MessageStop => break,
            _ => {}
        }
    }

    accumulated
} else {
    // Non-streaming path (existing logic)
    let response = self.client.messages().create(params).await?;
    // ... extract text as before ...
};
```

**Step 4: Update bot.rs to use streaming**

In `src/bot.rs`, modify the normal chat section (around line 347):

```rust
// Send initial placeholder message
let placeholder = bot
    .send_message(msg.chat.id, "...")
    .await
    .context("Failed to send placeholder")?;

let placeholder_id = placeholder.id;
let last_edit = Arc::new(Mutex::new(std::time::Instant::now()));

// Stream with callback that edits message
let stream_bot = bot.clone();
let stream_chat_id = msg.chat.id;

#[allow(clippy::cast_possible_wrap)]
let result = claude
    .chat(
        text,
        Some(uid as i64),
        Some(move |partial: &str| {
            let mut last = last_edit.lock().unwrap();
            if last.elapsed() >= std::time::Duration::from_millis(500) {
                let formatted = to_telegram_html(partial);
                let bot_clone = stream_bot.clone();
                let chat_id = stream_chat_id;
                let msg_id = placeholder_id;

                tokio::spawn(async move {
                    let _ = bot_clone
                        .edit_message_text(chat_id, msg_id, &format!("{}...", formatted))
                        .parse_mode(ParseMode::Html)
                        .await;
                });

                *last = std::time::Instant::now();
            }
        }),
    )
    .await;
```

**Step 5: Handle final message edit**

After chat completes, do final edit without "..." suffix:

```rust
// Final edit (remove "..." suffix)
let formatted_final = to_telegram_html(&response);
bot.edit_message_text(msg.chat.id, placeholder_id, &formatted_final)
    .parse_mode(ParseMode::Html)
    .await?;
```

**Step 6: Handle 4096-char limit**

Add helper function:

```rust
async fn send_long_message(
    bot: &Bot,
    chat_id: ChatId,
    text: &str,
    reply_to: Option<MessageId>,
) -> Result<()> {
    const MAX_LEN: usize = 4000; // Leave margin for formatting

    if text.len() <= MAX_LEN {
        let mut msg = bot.send_message(chat_id, text).parse_mode(ParseMode::Html);
        if let Some(reply_id) = reply_to {
            msg = msg.reply_to_message_id(reply_id);
        }
        msg.await?;
    } else {
        // Split at sentence boundaries near MAX_LEN
        let chunks = split_at_sentence(text, MAX_LEN);
        for (i, chunk) in chunks.iter().enumerate() {
            let mut msg = bot.send_message(chat_id, chunk).parse_mode(ParseMode::Html);
            if i == 0 {
                if let Some(reply_id) = reply_to {
                    msg = msg.reply_to_message_id(reply_id);
                }
            }
            msg.await?;
        }
    }

    Ok(())
}

fn split_at_sentence(text: &str, max_len: usize) -> Vec<String> {
    // Simple implementation: split at periods near max_len
    let mut chunks = Vec::new();
    let mut current = String::new();

    for sentence in text.split(". ") {
        if current.len() + sentence.len() > max_len && !current.is_empty() {
            chunks.push(current.clone());
            current.clear();
        }
        if !current.is_empty() {
            current.push_str(". ");
        }
        current.push_str(sentence);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}
```

**Step 7: Test streaming locally**

```bash
cargo build --release
./target/release/lu bot
```

Send long query in Telegram:
- Expected: See initial "..." message
- Expected: Message updates multiple times
- Expected: Final message has no "..." suffix

**Step 8: Commit**

```bash
git add src/claude.rs src/bot.rs
git commit -m "feat(telegram): add streaming response support with message editing"
```

---

### Task 2: Add Thread Support

**Files:**
- Modify: `src/bot.rs:140-400` (message handler)

**Context:** Thread support allows Lu to reply to specific messages, preserving conversation context in threads. Useful for multi-topic discussions.

**Step 1: Extract reply context from incoming message**

In message handler, after receiving text message:

```rust
// Get reply context if this is a reply
let reply_to_id = msg.reply_to_message().map(|m| m.id);
```

**Step 2: Pass reply context to send_message**

When sending response:

```rust
let mut send = bot
    .send_message(msg.chat.id, &formatted)
    .parse_mode(ParseMode::Html);

if let Some(reply_id) = reply_to_id {
    send = send.reply_to_message_id(reply_id);
}

send.await?;
```

**Step 3: Update streaming to support threads**

In streaming code, pass `reply_to_id` to placeholder message:

```rust
let mut placeholder_send = bot.send_message(msg.chat.id, "...");
if let Some(reply_id) = reply_to_id {
    placeholder_send = placeholder_send.reply_to_message_id(reply_id);
}
let placeholder = placeholder_send.await?;
```

**Step 4: Test thread support**

In Telegram:
1. Send a message to Lu
2. Reply to Lu's response with another question
3. Expected: Lu's next response also replies in thread

**Step 5: Commit**

```bash
git add src/bot.rs
git commit -m "feat(telegram): add thread support for replies"
```

---

### Task 3: Enhanced Error Reporting

**Files:**
- Modify: `src/tools/mod.rs` (error formatting)
- Modify: `src/tools/read_file.rs` (specific errors)
- Modify: `src/tools/search.rs` (specific errors)
- Modify: `src/mcp_client.rs` (MCP errors)

**Context:** Current errors are generic. Enhanced errors explain what went wrong and suggest corrective actions.

**Step 1: Create error formatter**

In `src/tools/mod.rs`, add:

```rust
/// Format a tool error with context and suggestions.
pub fn format_tool_error(tool_name: &str, error: &str) -> String {
    match tool_name {
        "read_file" if error.contains("not found") => {
            format!(
                "Unable to read file.\n\n\
                 Error: {error}\n\n\
                 Try:\n\
                 • Check the file path is relative to vault root\n\
                 • Use list_dir to see available files\n\
                 • File paths are case-sensitive"
            )
        }
        "search" if error.contains("empty") || error.contains("no results") => {
            format!(
                "No results found.\n\n\
                 Try:\n\
                 • Broaden your search query\n\
                 • Check spelling\n\
                 • Use vault_stats to see what content exists"
            )
        }
        _ => {
            // Generic error with tool name
            format!("Error in {tool_name}: {error}")
        }
    }
}
```

**Step 2: Update execute_tool_local to use formatter**

In `src/tools/mod.rs`, modify `execute_tool_local()`:

```rust
pub async fn execute_tool_local(name: &str, input: &Value, vault_path: &Path) -> String {
    let result = match name {
        "read_file" => read_file::execute(input, vault_path),
        "list_dir" => list_dir::execute(input, vault_path),
        "search" => search::execute(input, vault_path).await,
        // ... other tools ...
        _ => return format!("Unknown tool: {name}"),
    };

    match result {
        Ok(output) => output,
        Err(e) => format_tool_error(name, &e.to_string()),
    }
}
```

**Step 3: Add MCP-specific error formatting**

In `src/mcp_client.rs`, modify `call_tool()`:

```rust
pub async fn call_tool(&self, name: &str, input: &Value) -> Result<String> {
    // ... existing request code ...

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        // Format MCP errors helpfully
        return Ok(format!(
            "MCP connection error.\n\n\
             Status: {status}\n\
             Error: {body}\n\n\
             Try:\n\
             • Check MCP server is running on Mac\n\
             • Verify network connection\n\
             • Run /poke to test connection"
        ));
    }

    // ... rest of method ...
}
```

**Step 4: Test error messages**

Trigger various errors in Telegram:
- Request non-existent file: `read notes/missing.md`
- Search for nonsense: `search for asdfghjkl`
- Test with MCP server stopped

Expected: Helpful, actionable error messages

**Step 5: Commit**

```bash
git add src/tools/mod.rs src/mcp_client.rs
git commit -m "feat: add enhanced error reporting with suggestions"
```

---

### Task 4: Phase 2 Testing & Release

**Files:**
- Modify: `Cargo.toml:3` (version bump)

**Step 1: Run automated tests**

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

Expected: All pass

**Step 2: Manual Telegram testing**

```markdown
## Streaming Responses
- [ ] Long query → Message updates multiple times (every 500ms)
- [ ] Short query → Message updates once or twice
- [ ] Very long response (>4000 chars) → Splits into messages
- [ ] Final message has no "..." suffix
- [ ] No rate limit errors

## Thread Support
- [ ] Reply to Lu's message → Lu replies in thread
- [ ] Multiple parallel threads work independently

## Enhanced Error Reporting
- [ ] Request missing file → See helpful suggestions
- [ ] MCP down → See connection troubleshooting
- [ ] Invalid search → See query improvement tips
```

**Step 3: Bump version to 0.7.0**

```bash
sed -i '' 's/version = "0.6.0"/version = "0.7.0"/' Cargo.toml
git add Cargo.toml
git commit -m "chore: release v0.7.0"
```

**Step 4: Push and release**

```bash
git push origin develop
git push origin develop:production
gh release create v0.7.0 --title "v0.7.0 - Phase 2: Core UX" --notes "### Phase 2 Features

- feat(telegram): add streaming response support with message editing
- feat(telegram): add thread support for replies
- feat: add enhanced error reporting with suggestions

### Phase 2 Complete
All Phase 2 features tested and working on Telegram."
```

**Step 5: Deploy to Pi**

```bash
cd /tmp
gh release download v0.7.0 --repo evannagle/ludolph -p "lu-aarch64-unknown-linux-gnu" --clobber
scp lu-aarch64-unknown-linux-gnu pi:~/.ludolph/bin/lu.new
ssh pi "chmod +x ~/.ludolph/bin/lu.new && mv ~/.ludolph/bin/lu.new ~/.ludolph/bin/lu && systemctl --user restart ludolph.service"
ssh pi "~/.ludolph/bin/lu --version"
```

Expected: v0.7.0

---

## Phase 2 Exit Criteria

Before proceeding to Phase 3, verify:

- [ ] Streaming feels responsive (updates visible every 500ms)
- [ ] No Telegram rate limit errors from message editing
- [ ] Thread context preserved in replies
- [ ] Error messages are helpful and actionable
- [ ] All Phase 1 features still work (no regressions)
- [ ] Version 0.7.0 released and deployed
- [ ] User explicitly approves: "Phase 2 complete, proceed to Phase 3?"

---

## Summary

| Task | Feature | Estimated Effort | Test Method |
|------|---------|------------------|-------------|
| 1 | Streaming Responses | 3-4 hours | Telegram: Watch message update |
| 2 | Thread Support | 30 minutes | Telegram: Reply to messages |
| 3 | Enhanced Error Reporting | 2 hours | Telegram: Trigger various errors |
| 4 | Testing & Release | 1 hour | Manual checklist + deploy |

**Total estimated effort:** 6-8 hours
**Testing time:** 45-60 minutes
**Target version:** 0.7.0

---

## Sources

- [anthropic-sdk-rust documentation](https://docs.rs/anthropic-sdk-rust) - Streaming API reference
- [anthropic-sdk-rust crates.io](https://crates.io/crates/anthropic-sdk-rust) - Package details
