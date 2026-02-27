# Phase 1: Quick Wins Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add immediate UX improvements: status reactions, typing indicators, command auto-registration, and setup wizard polish.

**Architecture:** Enhance existing `src/bot.rs` with Telegram feedback mechanisms. All changes are additive - no breaking changes to existing functionality.

**Tech Stack:** Rust, teloxide 0.13, Telegram Bot API

---

### Task 1: Add Status Reactions

**Files:**
- Modify: `src/bot.rs:140-350` (message handler)

**Context:** Status reactions provide visual feedback using Telegram emoji reactions. User sees 👀 while processing, then ✅ on success or ❌ on error.

**Step 1: Add reaction helper function**

Add after line 19 (after imports):

```rust
/// Set a reaction on a message.
async fn set_reaction(
    bot: &Bot,
    chat_id: ChatId,
    message_id: MessageId,
    emoji: &str,
) -> Result<()> {
    let reaction = if emoji.is_empty() {
        vec![]
    } else {
        vec![ReactionType::Emoji {
            emoji: emoji.to_string(),
        }]
    };

    bot.set_message_reaction(chat_id, message_id)
        .reaction(reaction)
        .await
        .context("Failed to set reaction")?;

    Ok(())
}
```

**Step 2: Add reactions to message handler**

In the message handler (around line 175), after receiving a text message:

```rust
// Set "eyes" reaction to show we're processing
let _ = set_reaction(&bot, msg.chat.id, msg.id, "👀").await;
```

Before sending the response (around line 340), add:

```rust
// Change reaction to success/error
let reaction_emoji = if response_text.contains("error") || response_text.contains("Error") {
    "❌"
} else {
    "✅"
};
let _ = set_reaction(&bot, msg.chat.id, msg.id, reaction_emoji).await;
```

After sending response, clear reactions:

```rust
// Clear reaction after sending response
let _ = set_reaction(&bot, msg.chat.id, msg.id, "").await;
```

**Step 3: Test reactions**

Run locally:
```bash
cargo build --release
./target/release/lu bot
```

Send message in Telegram:
- Expected: See 👀 appear immediately
- Expected: See ✅ or ❌ when response arrives
- Expected: Reaction clears after response

**Step 4: Commit**

```bash
git add src/bot.rs
git commit -m "feat(telegram): add status reactions (👀/✅/❌)"
```

---

### Task 2: Add Typing Indicators

**Files:**
- Modify: `src/bot.rs:140-350` (message handler)

**Context:** Typing indicators show "Lu is typing..." in Telegram while processing. Telegram requires refreshing every 5 seconds.

**Step 1: Import ChatAction**

At top of file (around line 10), add to teloxide imports:

```rust
use teloxide::types::{ChatAction, ParseMode, ReactionType};
```

**Step 2: Add typing indicator helper**

Add after `set_reaction` function:

```rust
/// Send typing indicator (must be refreshed every 5s).
async fn send_typing(bot: &Bot, chat_id: ChatId) {
    let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;
}
```

**Step 3: Spawn background typing task**

In message handler, after setting 👀 reaction, add:

```rust
// Spawn background task to send typing indicators
let typing_bot = bot.clone();
let typing_chat_id = msg.chat.id;
let (typing_tx, mut typing_rx) = tokio::sync::mpsc::channel::<()>(1);

tokio::spawn(async move {
    loop {
        tokio::select! {
            _ = typing_rx.recv() => break,
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(5)) => {
                send_typing(&typing_bot, typing_chat_id).await;
            }
        }
    }
});
```

Before sending response, stop typing task:

```rust
// Stop typing indicator
drop(typing_tx);
```

**Step 4: Test typing indicators**

Run locally and send message:
- Expected: See "Lu is typing..." appear
- Expected: Typing persists for long processing
- Expected: Typing stops when response arrives

**Step 5: Commit**

```bash
git add src/bot.rs
git commit -m "feat(telegram): add typing indicators"
```

---

### Task 3: Enable Command Auto-Registration

**Files:**
- Modify: `src/bot.rs:47-67` (register_commands function)
- Modify: `src/bot.rs:100-110` (run_bot startup)

**Context:** Command registration already exists but isn't called. Need to invoke it on startup.

**Step 1: Check current registration function**

Run: `grep -A 20 "async fn register_commands" src/bot.rs`
Expected: Shows existing `register_commands()` function

**Step 2: Call registration on startup**

In `run_bot()` function (around line 100), after creating the bot:

```rust
// Register commands with Telegram
if let Err(e) = register_commands(&config.telegram.bot_token).await {
    tracing::warn!("Failed to register commands: {}", e);
    // Continue anyway - not critical for bot operation
}
```

**Step 3: Update command list**

Modify `register_commands()` to include `/cancel`:

```rust
{"command": "cancel", "description": "Cancel setup"},
```

**Step 4: Test auto-registration**

Restart bot on Pi:
```bash
ssh pi "systemctl --user restart ludolph.service"
```

In Telegram, type `/`:
- Expected: All 5 commands appear in autocomplete
- Expected: Descriptions match `/help` output

**Step 5: Commit**

```bash
git add src/bot.rs
git commit -m "feat(telegram): enable command auto-registration on startup"
```

---

### Task 4: Polish Setup Wizard

**Files:**
- Modify: `src/setup.rs:33-37` (acknowledgment guidance)

**Context:** Setup wizard improvements already in progress (4 commits). This task ensures Lu acknowledges user responses throughout the conversation.

**Step 1: Verify current state**

Run: `grep -A 5 "Ask Analysis Depth" src/setup.rs`
Expected: Shows contextual acknowledgment instructions

**Step 2: Add explicit acknowledgment reminder**

In SETUP_SYSTEM_PROMPT, after step 2 (Ask Analysis Depth), add:

```rust
   - IMPORTANT: Always reference what the user actually said, don't be generic
   - Bad: "Great choice!"
   - Good: "Perfect! I'll be your Friend and Research Partner as I help with [their stated use case]"
```

**Step 3: Test setup wizard**

Send `/setup` in Telegram:
1. Answer vault usage question
   - Expected: Lu references your specific answer
2. Pick persona(s)
   - Expected: Lu acknowledges with context: "I'll be X as I help with Y"
3. Choose analysis depth
   - Expected: Lu explains what it will analyze based on earlier responses

**Step 4: Commit**

```bash
git add src/setup.rs
git commit -m "docs(setup): add explicit acknowledgment guidelines"
```

---

### Task 5: Phase 1 Testing & Release

**Files:**
- Modify: `Cargo.toml:3` (version bump)
- New: Manual Telegram testing checklist

**Step 1: Run all automated tests**

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

Expected: All pass

**Step 2: Manual Telegram testing**

Use checklist from design doc:

```markdown
## Status Reactions
- [ ] Send "hello" → See 👀 appear → Changes to ✅
- [ ] Send invalid command → See ❌ reaction
- [ ] Verify reactions clear after response

## Typing Indicators
- [ ] Send message → See "Lu is typing..."
- [ ] Long query → Typing persists
- [ ] Response arrives → Typing stops

## Auto-Registration
- [ ] Type `/` in Telegram → All 5 commands appear
- [ ] Descriptions match `/help`

## Setup Wizard
- [ ] `/setup` flow feels natural and contextual
- [ ] Lu acknowledges responses specifically
- [ ] Lu.md created with vault insights
```

**Step 3: Bump version to 0.6.0**

```bash
sed -i '' 's/version = "0.5.6"/version = "0.6.0"/' Cargo.toml
```

**Step 4: Commit version bump**

```bash
git add Cargo.toml
git commit -m "chore: release v0.6.0"
```

**Step 5: Push and release**

```bash
git push origin develop
git push origin develop:production
gh release create v0.6.0 --title "v0.6.0 - Phase 1: Quick Wins" --notes "### Features

- feat(telegram): add status reactions (👀/✅/❌)
- feat(telegram): add typing indicators
- feat(telegram): enable command auto-registration
- fix(setup): improve contextual acknowledgments
- fix(claude): fix tool result API error

### Phase 1 Complete
All Phase 1 features tested and working on Telegram."
```

**Step 6: Monitor CI and deploy**

Wait for CI to complete, then deploy:
```bash
# Download Pi binary
cd /tmp
gh release download v0.6.0 --repo evannagle/ludolph -p "lu-aarch64-unknown-linux-gnu" --clobber

# Deploy to Pi
scp lu-aarch64-unknown-linux-gnu pi:~/.ludolph/bin/lu.new
ssh pi "chmod +x ~/.ludolph/bin/lu.new && mv ~/.ludolph/bin/lu.new ~/.ludolph/bin/lu && systemctl --user restart ludolph.service"

# Verify
ssh pi "~/.ludolph/bin/lu --version"
```

Expected: Shows v0.6.0

---

## Phase 1 Exit Criteria

Before proceeding to Phase 2, verify:

- [ ] All automated tests pass (cargo test)
- [ ] All Telegram test scenarios pass
- [ ] Version 0.6.0 released on GitHub
- [ ] Binary deployed to Pi
- [ ] No regressions (existing features work)
- [ ] User explicitly approves: "Phase 1 complete, proceed to Phase 2?"

---

## Summary

| Task | Feature | Lines Changed | Test Method |
|------|---------|---------------|-------------|
| 1 | Status Reactions | ~30 | Telegram: Send message, watch emoji |
| 2 | Typing Indicators | ~20 | Telegram: See "typing..." |
| 3 | Auto-Registration | ~5 | Telegram: Type `/` see commands |
| 4 | Setup Polish | ~10 | Telegram: Run `/setup` flow |
| 5 | Testing & Release | N/A | Manual checklist + release |

**Total estimated effort:** 4-6 hours
**Testing time:** 30-45 minutes
**Target version:** 0.6.0
