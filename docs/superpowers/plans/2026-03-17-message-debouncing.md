# Message Debouncing and Cancellation Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement intelligent message consolidation and request cancellation so rapid successive messages combine into a single LLM request, and users can cancel in-flight requests via `/cancel`.

**Architecture:** Per-user conversation state tracks pending messages and cancellation tokens. Cancellation checks occur at yield points: before each LLM call and between tool executions. Uses tokio::sync::Mutex for async-safe state access and tokio_util::CancellationToken for graceful abort.

**Tech Stack:** Rust, tokio, tokio-util (CancellationToken)

**Limitations:** True SSE streaming is not implemented in this phase. Cancellation responsiveness depends on tool execution frequency - simple questions may complete before cancellation fires, while multi-tool conversations provide frequent cancellation opportunities. Future work can add SSE streaming for sub-request cancellation.

**Design note:** The spec shows `state: ConversationState` passed directly to the LLM method. This plan uses a `check_new_messages: impl Fn() -> bool` closure instead, which decouples `llm.rs` from `bot.rs` types - a cleaner separation of concerns.

**Spec:** `docs/superpowers/specs/2026-03-17-message-debouncing-design.md`

---

## Chunk 1: Dependencies and Data Structures

### Task 1: Add Dependencies to Cargo.toml

**Files:**
- Modify: `Cargo.toml:39-44`

- [ ] **Step 1: Add tokio-util dependency**

Add after line 43 (after `eventsource-client`):

```toml
tokio-util = { version = "0.7", features = ["sync"] }
```

Note: `futures` and `reqwest` already exist in Cargo.toml.

- [ ] **Step 2: Verify dependencies resolve**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add tokio-util dependency for CancellationToken"
```

---

### Task 2: Add UserConversation Data Structure

**Files:**
- Modify: `src/bot.rs:1-26`

- [ ] **Step 1: Add new imports at top of file**

After line 7 (`use std::sync::{Arc, Mutex};`), add:

```rust
use std::collections::{HashMap, VecDeque};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;
```

Note: Keep existing `std::sync::Mutex` import - it's still used for `setup_users`.

- [ ] **Step 2: Add UserConversation struct after line 25**

After the existing imports (around line 25), add:

```rust
/// Tracks a user's pending messages and current processing state.
///
/// Used to consolidate rapid successive messages into a single LLM request
/// and enable cancellation of in-flight requests.
struct UserConversation {
    /// Messages waiting to be processed (or added mid-processing)
    pending: VecDeque<String>,
    /// Token to cancel current LLM request
    cancel_token: Option<CancellationToken>,
    /// Whether we're currently processing for this user
    processing: bool,
    /// Placeholder message ID for streaming edits
    placeholder_id: Option<MessageId>,
    /// Chat ID for this user (needed for cleanup)
    chat_id: Option<ChatId>,
}

impl Default for UserConversation {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            cancel_token: None,
            processing: false,
            placeholder_id: None,
            chat_id: None,
        }
    }
}

/// Global state: user_id -> conversation state.
///
/// Uses tokio::sync::Mutex (not std::sync::Mutex) because we need to hold
/// the lock across await points in some operations.
type ConversationState = Arc<AsyncMutex<HashMap<u64, UserConversation>>>;
```

- [ ] **Step 3: Verify compiles**

Run: `cargo check`
Expected: Compiles without errors (struct not used yet)

- [ ] **Step 4: Commit**

```bash
git add src/bot.rs
git commit -m "feat: add UserConversation struct for message debouncing"
```

---

### Task 3: Add consolidate_messages Function

**Files:**
- Modify: `src/bot.rs` (add after `UserConversation`)

- [ ] **Step 1: Write failing test**

Add at the bottom of `src/bot.rs` (before the closing `}`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consolidate_single_message() {
        let msgs = vec!["Hello".to_string()];
        assert_eq!(consolidate_messages(&msgs), "Hello");
    }

    #[test]
    fn test_consolidate_multiple_messages() {
        let msgs = vec!["Hello".to_string(), "How are you?".to_string()];
        let result = consolidate_messages(&msgs);
        assert!(result.contains("Message 1: Hello"));
        assert!(result.contains("Message 2: How are you?"));
        assert!(result.contains("Please respond to all"));
    }

    #[test]
    fn test_consolidate_empty_returns_empty() {
        let msgs: Vec<String> = vec![];
        assert_eq!(consolidate_messages(&msgs), "");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_consolidate --no-fail-fast`
Expected: FAIL with "cannot find function `consolidate_messages`"

- [ ] **Step 3: Write consolidate_messages function**

Add after the `ConversationState` type alias:

```rust
/// Consolidate multiple messages into a single prompt.
///
/// If there's only one message, returns it unchanged.
/// Multiple messages are formatted with explicit numbering so the LLM
/// understands they were sent in sequence.
fn consolidate_messages(messages: &[String]) -> String {
    match messages.len() {
        0 => String::new(),
        1 => messages[0].clone(),
        _ => {
            let mut prompt = String::from("[Multiple messages from user]\n\n");
            for (i, msg) in messages.iter().enumerate() {
                prompt.push_str(&format!("Message {}: {}\n", i + 1, msg));
            }
            prompt.push_str("\n---\n\nPlease respond to all of the above as a single conversation.");
            prompt
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_consolidate`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add src/bot.rs
git commit -m "feat: add consolidate_messages function for multi-message support"
```

---

## Chunk 2: LLM Streaming with Cancellation

### Task 4: Add chat_cancellable Method to Llm

**Files:**
- Modify: `src/llm.rs` (add after line 361)

- [ ] **Step 1: Add new imports at top of llm.rs**

After line 8 (`use serde_json::{Map, Value};`), add:

```rust
use tokio_util::sync::CancellationToken;
```

Note: No additional state types needed - the `check_new_messages` closure handles
state access without coupling llm.rs to bot.rs types.

- [ ] **Step 3: Add chat_cancellable method**

Add after `chat_streaming` method (after line 361):

```rust
    /// Chat with cancellation and new-message detection support.
    ///
    /// Cancellation is checked at yield points:
    /// - Before each LLM call
    /// - Between tool executions (after each tool completes)
    ///
    /// Note: During a single HTTP request to the MCP proxy, cancellation cannot
    /// interrupt mid-flight. Multi-tool conversations provide more frequent
    /// cancellation opportunities.
    ///
    /// # Returns
    /// - `Ok(Some(response))` - completed successfully
    /// - `Ok(None)` - cancelled by token OR new messages detected
    /// - `Err(e)` - error occurred
    ///
    /// # Errors
    ///
    /// Returns an error if the MCP server is unreachable or returns an error.
    pub async fn chat_cancellable<F>(
        &self,
        user_message: &str,
        user_id: Option<i64>,
        cancel_token: CancellationToken,
        check_new_messages: impl Fn() -> bool + Send,
        on_text: F,
    ) -> Result<Option<String>>
    where
        F: Fn(&str) + Send + Sync,
    {
        let tools = self.get_tools().await?;
        let mut messages = self.prepare_messages(user_message, user_id).await;

        tracing::debug!(
            "Starting cancellable chat with {} messages, {} tools",
            messages.len(),
            tools.len()
        );

        loop {
            // Check for cancellation before starting LLM request
            if cancel_token.is_cancelled() {
                tracing::debug!("Chat cancelled before LLM call");
                return Ok(None);
            }

            // Check for new messages before starting LLM request
            if check_new_messages() {
                tracing::debug!("New messages detected before LLM call");
                return Ok(None);
            }

            // Make the LLM call
            // Note: This HTTP request cannot be interrupted mid-flight
            let response = self.call_llm(&messages, &tools).await?;

            // Handle tool calls with cancellation checks between each tool
            if let Some(tool_calls) = &response.tool_calls {
                if !tool_calls.is_empty() {
                    tracing::debug!("Received {} tool calls", tool_calls.len());

                    // Add assistant message with tool calls
                    messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: ChatContent::Blocks(vec![serde_json::json!({
                            "type": "tool_use",
                            "tool_calls": tool_calls,
                        })]),
                    });

                    // Execute tools with cancellation checks between each
                    let mut results = Vec::new();
                    for tc in tool_calls {
                        // Check for cancellation between tools
                        if cancel_token.is_cancelled() {
                            tracing::debug!("Chat cancelled between tool executions");
                            return Ok(None);
                        }
                        if check_new_messages() {
                            tracing::debug!("New messages detected between tool executions");
                            return Ok(None);
                        }

                        let input: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default()));

                        tracing::debug!("Executing tool: {}", tc.function.name);
                        let result = self.execute_tool(&tc.function.name, &input).await;

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
            tracing::debug!("Chat complete, returning {} chars", content.len());

            // Call the callback with final result
            on_text(&content);

            self.store_message(user_id, "assistant", &content);
            return Ok(Some(content));
        }
    }
```

Note: This replaces the `handle_tool_calls` delegation with inline logic to enable
cancellation checks between individual tool executions.

- [ ] **Step 4: Verify compiles**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 5: Commit**

```bash
git add src/llm.rs
git commit -m "feat: add chat_cancellable with yield-point cancellation"
```

---

## Chunk 3: Message Handler Integration

### Task 5: Initialize ConversationState in run()

**Files:**
- Modify: `src/bot.rs:238-246`

- [ ] **Step 1: Add conversation_state initialization**

After line 239 (the `setup_users` initialization), add:

```rust
    // Track per-user conversation state for message debouncing
    let conversation_state: ConversationState = Arc::new(AsyncMutex::new(HashMap::new()));
```

- [ ] **Step 2: Clone for REPL closure**

After line 246 (inside the closure clones), add:

```rust
        let conversation_state = conversation_state.clone();
```

- [ ] **Step 3: Verify compiles**

Run: `cargo check`
Expected: Compiles (unused variable warning is OK for now)

- [ ] **Step 4: Commit**

```bash
git add src/bot.rs
git commit -m "feat: initialize ConversationState in bot run()"
```

---

### Task 6: Add process_user_conversation Function

**Files:**
- Modify: `src/bot.rs` (add before `run()` function)

- [ ] **Step 1: Add the processing function**

Add before the `pub async fn run()` function:

```rust
/// Process messages for a user with debouncing and cancellation support.
///
/// This function runs in a loop:
/// 1. Collects all pending messages
/// 2. Consolidates into single prompt
/// 3. Sends to LLM with cancellation support
/// 4. If new messages arrive during processing, restarts
/// 5. When complete (or cancelled), cleans up state
async fn process_user_conversation(
    user_id: u64,
    state: ConversationState,
    bot: Bot,
    llm: Llm,
    chat_id: ChatId,
) {
    loop {
        // Step 1: Get pending messages (short lock)
        let (messages, cancel_token, needs_placeholder) = {
            let mut guard = state.lock().await;
            let conv = match guard.get_mut(&user_id) {
                Some(c) => c,
                None => return, // User state was cleared
            };

            if conv.pending.is_empty() {
                // Nothing to process, we're done
                conv.processing = false;
                conv.cancel_token = None;
                return;
            }

            // Take all pending messages
            let msgs: Vec<String> = conv.pending.drain(..).collect();
            let token = conv.cancel_token.clone().unwrap_or_else(CancellationToken::new);
            let needs_ph = conv.placeholder_id.is_none();

            (msgs, token, needs_ph)
        }; // Lock dropped here

        // Step 2: Create placeholder OUTSIDE the lock (involves await)
        let placeholder_id = if needs_placeholder {
            let msg = bot.send_message(chat_id, "...").await.ok();
            let msg_id = msg.as_ref().map(|m| m.id);

            // Store placeholder ID (short lock)
            if let Some(id) = msg_id {
                let mut guard = state.lock().await;
                if let Some(conv) = guard.get_mut(&user_id) {
                    conv.placeholder_id = Some(id);
                }
            }
            msg_id
        } else {
            let guard = state.lock().await;
            guard.get(&user_id).and_then(|c| c.placeholder_id)
        };

        // Step 3: Consolidate messages
        let prompt = consolidate_messages(&messages);
        tracing::info!(
            "Processing {} message(s) for user {}: {:?}",
            messages.len(),
            user_id,
            if messages.len() == 1 {
                messages[0].chars().take(50).collect::<String>()
            } else {
                format!("{} messages", messages.len())
            }
        );

        // Step 4: Create closure to check for new messages
        let state_clone = state.clone();
        let check_new_messages = move || {
            // Use try_lock to avoid blocking - if we can't get lock, assume no new messages
            match state_clone.try_lock() {
                Ok(guard) => guard
                    .get(&user_id)
                    .map(|c| !c.pending.is_empty())
                    .unwrap_or(false),
                Err(_) => false,
            }
        };

        // Step 5: Process with cancellation
        #[allow(clippy::cast_possible_wrap)]
        let result = llm
            .chat_cancellable(
                &prompt,
                Some(user_id as i64),
                cancel_token.clone(),
                check_new_messages,
                |response| {
                    // Final response callback - update placeholder
                    if let Some(msg_id) = placeholder_id {
                        let formatted = to_telegram_html(response);
                        let bot_clone = bot.clone();

                        tokio::spawn(async move {
                            let _ = bot_clone
                                .edit_message_text(chat_id, msg_id, &formatted)
                                .parse_mode(ParseMode::Html)
                                .await;
                        });
                    }
                },
            )
            .await;

        match result {
            Ok(Some(response)) => {
                // Check if new messages arrived during processing
                let has_new = {
                    let guard = state.lock().await;
                    guard
                        .get(&user_id)
                        .map(|c| !c.pending.is_empty())
                        .unwrap_or(false)
                };

                if has_new {
                    // New messages arrived, restart with consolidated
                    tracing::info!("New messages arrived for user {}, restarting", user_id);
                    continue;
                }

                // Send final response
                if let Some(msg_id) = placeholder_id {
                    let formatted_final = to_telegram_html(&response);
                    let _ = bot
                        .edit_message_text(chat_id, msg_id, &formatted_final)
                        .parse_mode(ParseMode::Html)
                        .await;
                }

                // Clear state and exit
                {
                    let mut guard = state.lock().await;
                    if let Some(conv) = guard.get_mut(&user_id) {
                        conv.processing = false;
                        conv.cancel_token = None;
                        conv.placeholder_id = None;
                    }
                }

                // Mark success
                // Note: We can't access msg.id here, so skip reaction update
                tracing::info!("Chat complete for user {}", user_id);
                return;
            }
            Ok(None) => {
                // Cancelled or new messages detected during polling
                let has_new = {
                    let guard = state.lock().await;
                    guard
                        .get(&user_id)
                        .map(|c| !c.pending.is_empty())
                        .unwrap_or(false)
                };

                if has_new {
                    // Restart with new messages
                    tracing::info!("Restarting for user {} due to new messages", user_id);
                    continue;
                }

                // User cancelled, cleanup
                tracing::info!("Chat cancelled for user {}", user_id);
                if let Some(msg_id) = placeholder_id {
                    let _ = bot.delete_message(chat_id, msg_id).await;
                }

                {
                    let mut guard = state.lock().await;
                    if let Some(conv) = guard.get_mut(&user_id) {
                        conv.processing = false;
                        conv.cancel_token = None;
                        conv.placeholder_id = None;
                    }
                }
                return;
            }
            Err(e) => {
                // Error - report and cleanup
                tracing::error!("Chat error for user {}: {}", user_id, e);
                if let Some(msg_id) = placeholder_id {
                    let error_msg = format_api_error(&e);
                    let _ = bot
                        .edit_message_text(chat_id, msg_id, &error_msg)
                        .await;
                }

                {
                    let mut guard = state.lock().await;
                    if let Some(conv) = guard.get_mut(&user_id) {
                        conv.processing = false;
                        conv.cancel_token = None;
                        conv.placeholder_id = None;
                    }
                }
                return;
            }
        }
    }
}
```

- [ ] **Step 2: Verify compiles**

Run: `cargo check`
Expected: Compiles (may have unused warnings)

- [ ] **Step 3: Commit**

```bash
git add src/bot.rs
git commit -m "feat: add process_user_conversation for debounced chat"
```

---

### Task 7: Update Normal Chat Handler to Use Debouncing

**Files:**
- Modify: `src/bot.rs:459-560` (the "Normal chat with streaming" section)

- [ ] **Step 1: Replace the normal chat handler**

Replace the entire `else` block starting at line 459 (`} else {`) through line 560 with:

```rust
                } else {
                    // Normal chat with debouncing
                    tracing::info!("Received chat message from user {}: {}", uid, text);

                    // Add message to queue and potentially start processing
                    let should_spawn = {
                        let mut guard = conversation_state.lock().await;
                        let conv = guard.entry(uid).or_default();

                        // Store chat_id for later use
                        conv.chat_id = Some(msg.chat.id);

                        // Add message to pending queue
                        conv.pending.push_back(text.to_string());

                        // If already processing, message will be picked up by poller
                        if conv.processing {
                            tracing::debug!("User {} already processing, message queued", uid);
                            false
                        } else {
                            // Start processing
                            conv.processing = true;
                            conv.cancel_token = Some(CancellationToken::new());
                            true
                        }
                    }; // Lock dropped here before spawn

                    if should_spawn {
                        // Show processing indicators
                        set_reaction(&bot, msg.chat.id, msg.id, "👀").await;

                        // Spawn processing task
                        let state_clone = conversation_state.clone();
                        let bot_clone = bot.clone();
                        let llm_clone = llm.clone();
                        let chat_id = msg.chat.id;
                        let msg_id = msg.id;

                        tokio::spawn(async move {
                            process_user_conversation(
                                uid,
                                state_clone.clone(),
                                bot_clone.clone(),
                                llm_clone,
                                chat_id,
                            )
                            .await;

                            // Update reaction on completion
                            set_reaction(&bot_clone, chat_id, msg_id, "✅").await;
                            clear_reactions(&bot_clone, chat_id, msg_id).await;
                        });
                    } else {
                        // Message queued, show pending indicator
                        set_reaction(&bot, msg.chat.id, msg.id, "⏳").await;
                    }

                    // Return empty - response will be sent by processing task
                    String::new()
                };
```

- [ ] **Step 2: Verify compiles**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src/bot.rs
git commit -m "feat: integrate debouncing into normal chat handler"
```

---

## Chunk 4: Cancel Command

### Task 8: Update /cancel Command Handler

**Files:**
- Modify: `src/bot.rs:373-382` (the `/cancel` command handler)

- [ ] **Step 1: Replace the /cancel handler**

Replace lines 373-382 with:

```rust
                        "/cancel" => {
                            // Handle setup cancellation
                            if in_setup {
                                if let Ok(mut guard) = setup_users.lock() {
                                    guard.remove(&uid);
                                }
                                "Setup cancelled.".to_string()
                            } else {
                                // Handle chat cancellation
                                let result = {
                                    let mut guard = conversation_state.lock().await;
                                    if let Some(conv) = guard.get_mut(&uid) {
                                        if conv.processing {
                                            // Cancel current request
                                            if let Some(token) = &conv.cancel_token {
                                                token.cancel();
                                            }
                                            // Clear pending messages
                                            conv.pending.clear();
                                            // Return placeholder to delete
                                            conv.placeholder_id.take()
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                }; // Lock dropped here before await

                                if let Some(msg_id) = result {
                                    let _ = bot.delete_message(msg.chat.id, msg_id).await;
                                    "Cancelled.".to_string()
                                } else {
                                    "Nothing to cancel.".to_string()
                                }
                            }
                        }
```

- [ ] **Step 2: Verify compiles**

Run: `cargo check`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src/bot.rs
git commit -m "feat: update /cancel to support chat cancellation"
```

---

## Chunk 5: Final Integration and Testing

### Task 9: Run Full Test Suite

**Files:**
- None (verification only)

- [ ] **Step 1: Run cargo check**

Run: `cargo check`
Expected: No errors

- [ ] **Step 2: Run cargo clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run cargo test**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 4: Run cargo fmt**

Run: `cargo fmt`
Expected: Code formatted

- [ ] **Step 5: Final commit if any formatting changes**

```bash
git add -A
git commit -m "style: format code"
```

---

### Task 10: Manual Testing

**Files:**
- None (manual verification)

- [ ] **Step 1: Start the bot**

Run: `cargo run`
Expected: Bot starts successfully

- [ ] **Step 2: Test single message**

Send a message to the bot.
Expected: Normal response, ✅ reaction

- [ ] **Step 3: Test rapid messages**

Send "Hello" then immediately "How are you?" then "What's the weather?"
Expected: Single consolidated response addressing all three

- [ ] **Step 4: Test /cancel**

Send a long request, then `/cancel`.
Expected: "Cancelled." response, placeholder deleted

- [ ] **Step 5: Test queued message indicator**

Send a message while another is processing.
Expected: ⏳ reaction on queued message

---

### Task 11: Update /cancel Command Description

**Files:**
- Modify: `src/bot.rs:83` (in `register_commands`)

- [ ] **Step 1: Update command description**

Change line 83 from:
```rust
            {"command": "cancel", "description": "Cancel setup in progress"},
```
to:
```rust
            {"command": "cancel", "description": "Cancel current operation"},
```

- [ ] **Step 2: Commit**

```bash
git add src/bot.rs
git commit -m "docs: update /cancel description to reflect new capability"
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Add tokio-util dependency | Cargo.toml |
| 2 | Add UserConversation struct | src/bot.rs |
| 3 | Add consolidate_messages function | src/bot.rs |
| 4 | Add chat_cancellable method | src/llm.rs |
| 5 | Initialize ConversationState | src/bot.rs |
| 6 | Add process_user_conversation | src/bot.rs |
| 7 | Update chat handler for debouncing | src/bot.rs |
| 8 | Update /cancel handler | src/bot.rs |
| 9 | Run test suite | - |
| 10 | Manual testing | - |
| 11 | Update /cancel description | src/bot.rs |

Total: 11 tasks, ~30 steps
