# Telegram Progress Updates for Long Tasks

## Problem

When Ludolph processes a long request involving multiple tool calls, the Telegram placeholder message shows "..." with no updates until the entire operation completes. For tasks like writing a chapter or researching across many vault files, this can mean minutes of silence. The user has no way to tell if the bot is working or stuck.

## Solution

Channel-based progress reporting between the LLM layer and bot layer. The LLM sends progress events as it executes tools. The bot receives them and edits the placeholder message in-place. A 30-second debounce timer fires a "Still working..." update if a single LLM call takes long with no tool activity.

## Design

### Progress Event Enum

New type in `src/llm.rs`:

```rust
pub enum ProgressEvent {
    ToolStarted { name: String },
    ToolFinished { name: String },
    Done,
}
```

- `ToolStarted` — sent before each tool execution
- `ToolFinished` — sent after each tool execution, resets debounce timer
- `Done` — signals receiver to exit. The final response text is available from `chat_cancellable`'s return value; no need to duplicate it through the channel.

### LLM Changes (src/llm.rs)

`chat_cancellable` replaces the `on_text: F` generic callback with `progress_tx: mpsc::Sender<ProgressEvent>`. The `F` type parameter is removed.

In the tool execution loop, `ToolStarted` and `ToolFinished` events are sent around each tool call. On completion, `Done` is sent.

Channel send failures are logged at debug level and ignored — progress updates are cosmetic and must never abort tool execution.

### Bot Changes (src/bot.rs)

In `process_user_conversation`, before calling `chat_cancellable`:

1. Create channel: `mpsc::channel::<ProgressEvent>(8)`
2. If `placeholder_id` is `Some`, spawn receiver task with `bot`, `chat_id`, `placeholder_id`, `progress_rx`. If `None`, don't spawn; the receiver end is dropped and channel sends become no-ops.
3. Pass `progress_tx` to `chat_cancellable`
4. Channel drops when `chat_cancellable` returns; receiver exits naturally

The existing final `edit_message_text` after `chat_cancellable` returns stays unchanged — it overwrites whatever the receiver last wrote with the formatted final response.

### Receiver Task

The receiver loops on `progress_rx` with a 30-second timeout:

- `ToolStarted` — edit placeholder to human-friendly status (e.g., "Searching vault..."). Reset debounce.
- `ToolFinished` — reset debounce, no edit.
- 30s timeout with no events — edit placeholder to "Still working..."
- `Done` or channel closed — exit without editing. The caller handles the final message.

The initial 30s timeout also applies before any tool calls start. If the first LLM call takes over 30 seconds (just thinking, no tools), the debounce fires "Still working..." which is the intended behavior.

On cancellation, there is a benign race where the receiver may try to edit a message that the caller just deleted. This produces a Telegram API 400 error that is silently ignored.

### Tool Display Names

A mapping function for human-friendly status text:

```rust
fn tool_display_name(name: &str) -> &str {
    match name {
        "search" => "Searching vault",
        "read_file" => "Reading file",
        "list_dir" => "Browsing vault",
        "save_observation" => "Noting that",
        "search_observations" => "Checking memory",
        _ => "Working",
    }
}
```

Unknown tools display "Working". No file paths or arguments in the status.

## Files Changed

- `src/llm.rs` — `ProgressEvent` enum, modified `chat_cancellable` signature and tool loop
- `src/bot.rs` — receiver task, channel creation, `tool_display_name`

## Not In Scope

- Streaming partial LLM text (would require SSE support from MCP proxy)
- Multiple Telegram messages (all updates edit the single placeholder)
- Progress bars or percentage indicators
