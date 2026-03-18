# Message Debouncing and Cancellation

Design spec for implementing intelligent message consolidation and request cancellation in the Ludolph Telegram bot.

## Summary

When users send rapid successive messages, the bot should consolidate them into a single LLM request rather than spawning multiple parallel requests. Additionally, users can cancel in-flight requests via `/cancel` command. The system uses polling-based detection during processing to achieve responsive cancellation similar to Claude Code's ESC key behavior.

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│                        Telegram Bot                             │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │              ConversationState (per-user)                 │  │
│  │  ┌─────────────────────────────────────────────────────┐ │  │
│  │  │ UserConversation {                                   │ │  │
│  │  │   pending: VecDeque<String>  // queued messages      │ │  │
│  │  │   cancel_token: CancellationToken                    │ │  │
│  │  │   processing: bool                                   │ │  │
│  │  │   placeholder_id: Option<MessageId>                  │ │  │
│  │  │ }                                                    │ │  │
│  │  └─────────────────────────────────────────────────────┘ │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│                              ▼                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                   Processing Loop                         │  │
│  │                                                           │  │
│  │   1. Consolidate pending → single prompt                  │  │
│  │   2. Start LLM request with cancellation                  │  │
│  │   3. Poll every 500ms:                                    │  │
│  │      - LLM done? → send response                          │  │
│  │      - Cancelled? → abort, cleanup                        │  │
│  │      - New messages? → cancel, restart loop               │  │
│  │                                                           │  │
│  └──────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────┘
```

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Debounce approach | Poll during processing | No upfront latency; check for new messages while working |
| Poll interval | 500ms | Responsive without excessive overhead |
| Message consolidation | Explicit format | Clear to LLM which messages were sent in sequence |
| Cancellation trigger | `/cancel` command | Explicit user intent, no accidental cancellation |
| State scope | Per-user | Each user's conversation is independent |
| Streaming requirement | True SSE | Required for responsive cancellation (fix TODO in `src/llm.rs:355-361`) |
| Mutex type | tokio::sync::Mutex | Async-aware mutex for code involving await points |
| Graceful degradation | Fall back to sync | If SSE fails, use non-streaming (less responsive but works) |

## Components

### 1. Data Structures

Location: `src/bot.rs`

```rust
use std::collections::{HashMap, VecDeque};
use tokio::sync::Mutex; // Async-aware mutex for use with await points
use tokio_util::sync::CancellationToken;

/// Tracks a user's pending messages and current processing state
struct UserConversation {
    /// Messages waiting to be processed (or added mid-processing)
    pending: VecDeque<String>,
    /// Token to cancel current LLM request
    cancel_token: Option<CancellationToken>,
    /// Whether we're currently processing for this user
    processing: bool,
    /// Placeholder message ID for streaming edits
    placeholder_id: Option<MessageId>,
}

impl Default for UserConversation {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            cancel_token: None,
            processing: false,
            placeholder_id: None,
        }
    }
}

/// Global state: user_id -> conversation state
/// Uses tokio::sync::Mutex (not std::sync::Mutex) because we need to hold
/// the lock across await points in some operations.
type ConversationState = Arc<Mutex<HashMap<u64, UserConversation>>>;
```

### 2. Message Handler Changes

Location: `src/bot.rs`, within teloxide REPL handler

When a new message arrives:

```rust
// Get or create user conversation state
let should_spawn = {
    let mut state = conversation_state.lock().await; // tokio::sync::Mutex uses .await
    let conv = state.entry(user_id).or_default();

    // Add message to pending queue
    conv.pending.push_back(text.to_string());

    // If already processing, message will be picked up by poller
    if conv.processing {
        false
    } else {
        // Start processing
        conv.processing = true;
        conv.cancel_token = Some(CancellationToken::new());
        true
    }
}; // Lock dropped here before spawn

if should_spawn {
    // Spawn processing task (see Section 3)
    tokio::spawn(process_user_conversation(
        user_id,
        conversation_state.clone(),
        bot.clone(),
        llm.clone(),
        chat_id,
    ));
}

// Return empty - response will be sent by processing task
String::new()
```

### 3. Processing Task

Location: `src/bot.rs`

```rust
use tokio::sync::mpsc;

async fn process_user_conversation(
    user_id: u64,
    state: ConversationState,
    bot: Bot,
    llm: Llm,
    chat_id: ChatId,
) {
    loop {
        // Step 1: Get pending messages (short lock, no await)
        let (messages, cancel_token, needs_placeholder) = {
            let mut guard = state.lock().await;
            let conv = guard.get_mut(&user_id).unwrap();

            if conv.pending.is_empty() {
                // Nothing to process, we're done
                conv.processing = false;
                conv.cancel_token = None;
                return;
            }

            // Take all pending messages
            let msgs: Vec<String> = conv.pending.drain(..).collect();
            let token = conv.cancel_token.clone().unwrap();
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

        // Step 4: Create channel for streaming updates (avoids async in callback)
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let edit_bot = bot.clone();
        let edit_chat_id = chat_id;
        let edit_msg_id = placeholder_id;

        // Spawn task to handle streaming edits with rate limiting
        let edit_handle = tokio::spawn(async move {
            let mut last_edit = std::time::Instant::now();
            while let Some(partial) = rx.recv().await {
                // Rate limit edits to avoid Telegram API limits
                if last_edit.elapsed() >= std::time::Duration::from_millis(500) {
                    if let Some(msg_id) = edit_msg_id {
                        let _ = edit_bot.edit_message_text(edit_chat_id, msg_id, &partial)
                            .parse_mode(ParseMode::Html)
                            .await;
                    }
                    last_edit = std::time::Instant::now();
                }
            }
        });

        // Step 5: Process with cancellation and new-message polling
        let result = llm.chat_streaming_cancellable(
            &prompt,
            Some(user_id as i64),
            cancel_token.clone(),
            state.clone(),
            user_id,
            move |partial| {
                // Send partial to edit task (non-blocking)
                let _ = tx.send(partial.to_string());
            },
        ).await;

        // Stop edit task
        drop(tx);
        let _ = edit_handle.await;

        match result {
            Ok(Some(response)) => {
                // Check if new messages arrived during processing
                let has_new = {
                    let guard = state.lock().await;
                    guard.get(&user_id)
                        .map(|c| !c.pending.is_empty())
                        .unwrap_or(false)
                };

                if has_new {
                    // New messages arrived, restart with consolidated
                    continue;
                }

                // Send final response
                if let Some(msg_id) = placeholder_id {
                    let _ = bot.edit_message_text(chat_id, msg_id, &response)
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
                return;
            }
            Ok(None) => {
                // Cancelled or new messages detected during polling
                let has_new = {
                    let guard = state.lock().await;
                    guard.get(&user_id)
                        .map(|c| !c.pending.is_empty())
                        .unwrap_or(false)
                };

                if has_new {
                    // Restart with new messages
                    continue;
                }

                // User cancelled, cleanup
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
                if let Some(msg_id) = placeholder_id {
                    let _ = bot.edit_message_text(chat_id, msg_id, &format!("Error: {e}"))
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

### 4. Message Consolidation Format

```rust
fn consolidate_messages(messages: &[String]) -> String {
    if messages.len() == 1 {
        return messages[0].clone();
    }

    let mut prompt = String::from("[Multiple messages from user]\n\n");
    for (i, msg) in messages.iter().enumerate() {
        prompt.push_str(&format!("Message {}: {}\n", i + 1, msg));
    }
    prompt.push_str("\n---\n\nPlease respond to all of the above as a single conversation.");
    prompt
}
```

### 5. LLM Cancellation Support

Location: `src/llm.rs`

New dependencies in Cargo.toml:
```toml
reqwest-eventsource = "0.6"
tokio-util = { version = "0.7", features = ["sync"] }
futures = "0.3"
```

```rust
use futures::StreamExt; // Required for es.next()
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use tokio_util::sync::CancellationToken;

impl Llm {
    /// Chat with streaming, cancellation, and new-message polling support
    ///
    /// Returns:
    /// - Ok(Some(response)) - completed successfully
    /// - Ok(None) - cancelled by token OR new messages detected
    /// - Err(e) - error occurred
    pub async fn chat_streaming_cancellable<F>(
        &self,
        user_message: &str,
        user_id: Option<i64>,
        cancel_token: CancellationToken,
        state: ConversationState,
        check_user_id: u64,
        on_partial: F,
    ) -> Result<Option<String>>
    where
        F: Fn(&str),
    {
        let url = format!("{}/chat/stream", self.base_url);

        // Build request and convert to EventSource
        let request = self.client
            .post(&url)
            .json(&serde_json::json!({
                "message": user_message,
                "user_id": user_id
            }));

        let mut es = request.eventsource()?;

        let mut accumulated = String::new();
        let mut poll_interval = tokio::time::interval(Duration::from_millis(500));
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                biased; // Check cancellation first

                // Check for explicit cancellation (e.g., /cancel command)
                _ = cancel_token.cancelled() => {
                    es.close();
                    return Ok(None);
                }

                // Poll for new messages every 500ms
                _ = poll_interval.tick() => {
                    // Check if new messages arrived while we're processing
                    let has_new = {
                        let guard = state.lock().await;
                        guard.get(&check_user_id)
                            .map(|c| !c.pending.is_empty())
                            .unwrap_or(false)
                    };

                    if has_new {
                        // New messages detected - cancel and let caller restart
                        es.close();
                        return Ok(None);
                    }
                }

                // Process SSE events
                event = es.next() => {
                    match event {
                        Some(Ok(Event::Message(msg))) => {
                            if msg.event == "content" {
                                accumulated.push_str(&msg.data);
                                on_partial(&accumulated);
                            } else if msg.event == "done" {
                                return Ok(Some(accumulated));
                            } else if msg.event == "error" {
                                return Err(anyhow::anyhow!("LLM error: {}", msg.data));
                            }
                        }
                        Some(Ok(Event::Open)) => {
                            tracing::debug!("SSE connection opened");
                        }
                        Some(Err(e)) => {
                            // Connection error, fall back to sync
                            tracing::warn!("SSE error, falling back to sync: {e}");
                            return self.chat(user_message, user_id).await.map(Some);
                        }
                        None => {
                            // Stream ended normally
                            return Ok(Some(accumulated));
                        }
                    }
                }
            }
        }
    }
}
```

**Key implementation notes:**

1. Uses `RequestBuilderExt::eventsource()` from reqwest-eventsource for proper SSE handling
2. `biased` in select! ensures cancellation is checked first
3. Poll interval checks ConversationState for new messages every 500ms
4. Returns `Ok(None)` for both explicit cancellation AND new-message detection
5. Caller distinguishes by checking `pending.is_empty()` after return

### 6. /cancel Command Handler

Location: `src/bot.rs`, in command handler

```rust
"/cancel" => {
    let result = {
        let mut guard = conversation_state.lock().await;
        if let Some(conv) = guard.get_mut(&user_id) {
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
        let _ = bot.delete_message(chat_id, msg_id).await;
        "Cancelled.".to_string()
    } else {
        "Nothing to cancel.".to_string()
    }
}
```

## Error Handling

| Scenario | Behavior |
|----------|----------|
| LLM request fails | Show error, clear processing state, user can retry |
| SSE connection drops | Fall back to sync request, log warning |
| Cancellation during tool execution | Wait for current tool to finish (can't interrupt MCP mid-call) |
| Message arrives during error state | Start fresh processing with new message |
| Bot restart mid-conversation | Processing state lost (acceptable - user retries) |

## Testing

### Unit Tests

```rust
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
}

#[tokio::test]
async fn test_cancel_token_stops_request() {
    let token = CancellationToken::new();
    let token_clone = token.clone();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        token_clone.cancel();
    });

    let result = some_cancellable_operation(token).await;
    assert!(result.is_none()); // Cancelled
}
```

### Integration Tests

1. Send rapid message sequence, verify single consolidated response
2. Send `/cancel` during processing, verify clean abort
3. Verify streaming edits still work with new architecture

### Manual Test Scenarios

1. Send "Hello" then immediately "Can you help?" - should get one response addressing both
2. Send long request, then `/cancel` - should stop quickly
3. Send 5 messages in 3 seconds - should consolidate all 5

## Files Changed

| File | Change |
|------|--------|
| `src/bot.rs` | Add ConversationState, UserConversation, processing loop, /cancel handler |
| `src/llm.rs` | Add chat_streaming_cancellable with SSE client |
| `Cargo.toml` | Add reqwest-eventsource, tokio-util dependencies |

## Dependencies Added

```toml
[dependencies]
reqwest-eventsource = "0.6"
tokio-util = { version = "0.7", features = ["sync"] }
futures = "0.3"
```

## Not Included (YAGNI)

- Message priority (all messages equal)
- Per-user debounce configuration
- Cross-device message coordination
- Message editing (only new messages trigger consolidation)
- Partial cancellation (cancel specific messages)
