# Telegram Progress Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show real-time tool-call progress in Telegram during long-running LLM conversations, with a 30-second debounce for idle stretches.

**Architecture:** An mpsc channel carries `ProgressEvent` values from the LLM tool loop to a bot-side receiver task. The receiver edits the existing placeholder Telegram message in-place. A 30-second `tokio::time::timeout` on the channel receive fires a "Still working..." heartbeat when the LLM is idle.

**Tech Stack:** Rust, tokio (mpsc, time), teloxide (edit_message_text)

**Spec:** `docs/superpowers/specs/2026-04-14-telegram-progress-updates-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/llm.rs` | Modify | `ProgressEvent` enum, `chat_cancellable` signature change, send events in tool loop |
| `src/bot.rs` | Modify | `tool_display_name` fn, progress receiver task, channel wiring in `process_user_conversation` |

---

## Task 1: Add `ProgressEvent` enum and update `chat_cancellable` signature

**Files:**
- Modify: `src/llm.rs:1-10` (add `use tokio::sync::mpsc`)
- Modify: `src/llm.rs:737-836` (`chat_cancellable`)

- [ ] **Step 1: Add `ProgressEvent` enum and mpsc import**

At the top of `src/llm.rs`, add:

```rust
use tokio::sync::mpsc;
```

After the `SetupChatResult` struct (around line 27), add:

```rust
/// Progress events sent from the LLM tool loop to the bot layer.
#[derive(Debug)]
pub enum ProgressEvent {
    /// A tool is about to execute.
    ToolStarted { name: String },
    /// A tool finished executing.
    ToolFinished { name: String },
    /// The LLM conversation is complete.
    Done,
}
```

- [ ] **Step 2: Update `chat_cancellable` signature**

Replace the current signature (lines 737-747):

```rust
pub async fn chat_cancellable<F, C>(
    &self,
    user_message: &str,
    user_id: Option<i64>,
    cancel_token: CancellationToken,
    check_new_messages: C,
    on_text: F,
) -> Result<Option<String>>
where
    F: Fn(&str) + Send + Sync,
    C: Fn() -> bool + Send,
```

With:

```rust
pub async fn chat_cancellable<C>(
    &self,
    user_message: &str,
    user_id: Option<i64>,
    cancel_token: CancellationToken,
    check_new_messages: C,
    progress_tx: mpsc::Sender<ProgressEvent>,
) -> Result<Option<String>>
where
    C: Fn() -> bool + Send,
```

- [ ] **Step 3: Add progress sends in the tool loop**

In the tool execution loop (around lines 791-816), wrap each tool call with progress events. Replace the loop body:

```rust
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

    let input: Value = serde_json::from_str(&tc.function.arguments)
        .unwrap_or_else(|_| Value::Object(Map::default()));

    tracing::debug!("Executing tool: {}", tc.function.name);

    if progress_tx
        .send(ProgressEvent::ToolStarted {
            name: tc.function.name.clone(),
        })
        .await
        .is_err()
    {
        tracing::debug!("Progress receiver dropped, continuing");
    }

    let result = self.execute_tool(&tc.function.name, &input, user_id).await;

    if progress_tx
        .send(ProgressEvent::ToolFinished {
            name: tc.function.name.clone(),
        })
        .await
        .is_err()
    {
        tracing::debug!("Progress receiver dropped, continuing");
    }

    // Track file access for focus layer
    self.track_file_access(user_id, &tc.function.name, &input, &result);

    results.push(serde_json::json!({
        "type": "tool_result",
        "tool_use_id": tc.id,
        "content": result,
    }));
}
```

- [ ] **Step 4: Replace `on_text` call with `Done` event**

Replace lines 830-831:

```rust
// Call the callback with final result
on_text(&content);
```

With:

```rust
let _ = progress_tx.send(ProgressEvent::Done).await;
```

- [ ] **Step 5: Verify it compiles (expect bot.rs errors)**

Run: `cargo check 2>&1 | head -30`

Expected: Errors in `src/bot.rs` because the call site still passes a closure. The `src/llm.rs` changes should be clean.

- [ ] **Step 6: Commit**

```bash
git add src/llm.rs
git commit -m "feat: add ProgressEvent enum and update chat_cancellable signature"
```

---

## Task 2: Add `tool_display_name` and progress receiver to bot

**Files:**
- Modify: `src/bot.rs:1-17` (add imports)
- Modify: `src/bot.rs` (add `tool_display_name` function and `spawn_progress_receiver` function)

- [ ] **Step 1: Write test for `tool_display_name`**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `src/bot.rs`:

```rust
#[test]
fn tool_display_name_maps_known_tools() {
    assert_eq!(tool_display_name("search"), "Searching vault");
    assert_eq!(tool_display_name("search_index"), "Searching vault");
    assert_eq!(tool_display_name("read_file"), "Reading file");
    assert_eq!(tool_display_name("list_dir"), "Browsing vault");
    assert_eq!(tool_display_name("vault_map"), "Mapping vault");
    assert_eq!(tool_display_name("save_observation"), "Noting that");
    assert_eq!(tool_display_name("search_observations"), "Checking memory");
    assert_eq!(tool_display_name("create_file"), "Writing to vault");
    assert_eq!(tool_display_name("append_file"), "Writing to vault");
}

#[test]
fn tool_display_name_returns_working_for_unknown() {
    assert_eq!(tool_display_name("some_new_tool"), "Working");
    assert_eq!(tool_display_name(""), "Working");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test tool_display_name -- --nocapture 2>&1`

Expected: FAIL — function not found.

- [ ] **Step 3: Implement `tool_display_name`**

Add this function in `src/bot.rs`, after the `consolidate_messages` function (around line 105):

```rust
/// Map tool names to human-friendly progress text.
fn tool_display_name(name: &str) -> &str {
    match name {
        "search" | "search_index" => "Searching vault",
        "read_file" => "Reading file",
        "list_dir" => "Browsing vault",
        "vault_map" => "Mapping vault",
        "save_observation" => "Noting that",
        "search_observations" => "Checking memory",
        "create_file" | "append_file" => "Writing to vault",
        _ => "Working",
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test tool_display_name -- --nocapture 2>&1`

Expected: PASS (both tests).

- [ ] **Step 5: Add imports**

Add to the import block at the top of `src/bot.rs`:

```rust
use tokio::sync::mpsc;
use crate::llm::ProgressEvent;
```

Also update the existing `tokio::time` import (line 17) from:

```rust
use tokio::time::{Duration, interval};
```

to:

```rust
use tokio::time::{Duration, interval, timeout};
```

- [ ] **Step 6: Implement `spawn_progress_receiver`**

Add this function in `src/bot.rs`, after `tool_display_name`:

```rust
/// Spawn a task that receives progress events and edits the Telegram placeholder.
///
/// Edits the placeholder message on `ToolStarted` events with a human-friendly
/// status. If no events arrive for 30 seconds, edits to "Still working..."
/// Exits when the channel closes or `Done` is received.
fn spawn_progress_receiver(
    bot: Bot,
    chat_id: ChatId,
    placeholder_id: MessageId,
    mut rx: mpsc::Receiver<ProgressEvent>,
) {
    tokio::spawn(async move {
        loop {
            match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
                Ok(Some(ProgressEvent::ToolStarted { name })) => {
                    let status = format!("{}...", tool_display_name(&name));
                    let _ = bot.edit_message_text(chat_id, placeholder_id, &status).await;
                }
                Ok(Some(ProgressEvent::ToolFinished { .. })) => {
                    // Reset debounce by continuing the loop
                }
                Ok(Some(ProgressEvent::Done) | None) => {
                    // Done or channel closed — exit without editing
                    break;
                }
                Err(_) => {
                    // 30s timeout — send heartbeat
                    let _ = bot
                        .edit_message_text(chat_id, placeholder_id, "Still working...")
                        .await;
                }
            }
        }
    });
}
```

- [ ] **Step 7: Commit**

```bash
git add src/bot.rs
git commit -m "feat: add tool_display_name and progress receiver task"
```

---

## Task 3: Wire channel into `process_user_conversation`

**Files:**
- Modify: `src/bot.rs:312-335` (the `chat_cancellable` call site)

- [ ] **Step 1: Replace the `chat_cancellable` call**

Replace lines 312-335 (from `// Step 5: Process with cancellation` through `.await;`):

```rust
        // Step 5: Create progress channel and spawn receiver
        let (progress_tx, progress_rx) = mpsc::channel::<ProgressEvent>(8);

        if let Some(msg_id) = placeholder_id {
            spawn_progress_receiver(bot.clone(), chat_id, msg_id, progress_rx);
        }
        // If placeholder_id is None, progress_rx is dropped and sends become no-ops

        // Step 6: Process with cancellation
        #[allow(clippy::cast_possible_wrap)]
        let result = llm
            .chat_cancellable(
                &prompt,
                Some(user_id as i64),
                cancel_token.clone(),
                check_new_messages,
                progress_tx,
            )
            .await;
```

- [ ] **Step 2: Verify full build passes**

Run: `cargo check 2>&1`

Expected: No errors. The `on_text` closure is removed and replaced by the channel.

- [ ] **Step 3: Run all tests**

Run: `cargo test 2>&1`

Expected: All tests pass, including existing bot tests and the new `tool_display_name` tests.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`

Expected: No warnings.

- [ ] **Step 5: Commit**

```bash
git add src/bot.rs
git commit -m "feat: wire progress channel into process_user_conversation"
```

---

## Task 4: Final verification

- [ ] **Step 1: Run full pre-push checks**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test 2>&1`

Expected: All pass.

- [ ] **Step 2: Review diff**

Run: `git diff main --stat`

Expected: Only `src/llm.rs` and `src/bot.rs` changed, plus the spec and plan docs.

- [ ] **Step 3: Commit spec and plan docs**

```bash
git add docs/superpowers/specs/2026-04-14-telegram-progress-updates-design.md
git add docs/superpowers/plans/2026-04-14-telegram-progress-updates.md
git commit -m "docs: add spec and plan for telegram progress updates"
```
