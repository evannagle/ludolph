# Phased Implementation Roadmap Design

OpenClaw-inspired enhancements for Ludolph, implemented in three testable phases.

## Overview

Enhance Ludolph with openclaw's best patterns across three phases, with Telegram testing gates between each phase. Each phase is self-contained with clear acceptance criteria.

## Testing Strategy

**Phase Structure:**

```
Phase 1: Quick Wins (1-2 days)
  → Implement 4 features
  → Test on Telegram
  → Release v0.6.0
  → GATE: All Phase 1 tests pass

Phase 2: Core UX (3-5 days)
  → Implement 3 features
  → Test on Telegram
  → Release v0.7.0
  → GATE: All Phase 2 tests pass

Phase 3: Semantic Memory (1-2 weeks)
  → Implement 3 features
  → Test on Telegram
  → Release v0.8.0
```

**Between-Phase Gates:**
1. Automated tests pass (cargo test)
2. Manual Telegram testing complete (checklist)
3. Release deployed to Pi
4. Explicit user approval to proceed

## Phase 1: Quick Wins

**Goal:** Immediate UX improvements with minimal complexity

**Features:**

| Feature | Impact | Effort | Files |
|---------|--------|--------|-------|
| Status Reactions | HIGH | LOW | `src/bot.rs` |
| Typing Indicators | MEDIUM | LOW | `src/bot.rs` |
| Command Auto-Registration | LOW | LOW | `src/bot.rs` |
| Setup Wizard Polish | MEDIUM | LOW | `src/setup.rs` (in progress) |

### 1.1 Status Reactions

**What:** Set emoji reactions on user messages to show processing state

```
User sends: "What's in my vault?"
  → Lu sets 👀 reaction (processing)
  → Lu generates response
  → Lu changes to ✅ (success) or ❌ (error)
```

**Implementation:**
- After receiving message: `bot.set_message_reaction(chat_id, message_id, "👀")`
- Before sending response: Change to `"✅"` (success) or `"❌"` (error)
- Clear reactions on final send: `bot.set_message_reaction(chat_id, message_id, [])`

**Dependencies:** teloxide 0.13 (has reaction support)

### 1.2 Typing Indicators

**What:** Send "typing..." chat action while processing

**Implementation:**
- After receiving message: `bot.send_chat_action(chat_id, ChatAction::Typing)`
- Repeat every 5 seconds while processing (Telegram timeout is 5s)
- Stop when sending response

**Pattern:** Spawn background task that sends typing action in loop until response ready

### 1.3 Command Auto-Registration

**What:** Call Telegram `setMyCommands` API on bot startup

**Implementation:**
- In `run_bot()` startup, call:
```rust
bot.set_my_commands([
    BotCommand::new("setup", "Configure your assistant"),
    BotCommand::new("version", "Show version info"),
    BotCommand::new("poke", "Show connection and available tools"),
    BotCommand::new("cancel", "Cancel setup"),
    BotCommand::new("help", "Show available commands"),
]).await?;
```

### 1.4 Setup Wizard Polish

**Status:** Already improved (3 commits today)
**Remaining:** Ensure Lu acknowledges responses contextually throughout conversation

### Testing Phase 1

**Telegram Test Scenarios:**

```markdown
## Status Reactions
1. Send "hello" → See 👀 appear → Changes to ✅ when response arrives
2. Send "/invalid" → See 👀 appear → Changes to ❌
3. Verify reactions clear after response sent

## Typing Indicators
1. Send "search my vault for X" → See "Lu is typing..."
2. Long query → Typing persists until response
3. Quick query → Typing appears briefly

## Auto-Registration
1. Restart bot on Pi
2. Open Telegram, type `/` → All 5 commands appear
3. Check descriptions match `/help` output

## Setup Wizard
1. Send `/setup`
2. Respond to vault usage question → Lu references your answer
3. Pick persona → Lu acknowledges contextually
4. Choose analysis depth → Lu describes what it will look for
5. Complete → Lu.md created with specific vault insights
```

**Exit Criteria:**
- [ ] All Telegram tests pass
- [ ] cargo test passes (32/32)
- [ ] Version 0.6.0 released
- [ ] Deployed to Pi
- [ ] User approval to proceed

Does Phase 1 detail look right?

## Phase 2: Core UX

**Goal:** Responsive, engaging conversation experience

**Features:**

| Feature | Impact | Effort | Files |
|---------|--------|--------|-------|
| Streaming Responses | HIGH | MEDIUM | `src/bot.rs`, `src/claude.rs` |
| Thread Support | LOW | LOW | `src/bot.rs` |
| Enhanced Error Reporting | MEDIUM | MEDIUM | `src/tools/mod.rs`, `src/claude.rs` |

### 2.1 Streaming Responses

**What:** Edit Telegram message in real-time as Claude generates response

**Flow:**
1. User sends query
2. Lu sends initial message: "Thinking..."
3. As Claude streams tokens, Lu edits message every 500ms
4. Final edit contains complete response

**Implementation:**
- Modify `src/claude.rs::chat()` to support streaming callback
- In `src/bot.rs`, send initial message and store message ID
- In streaming callback: Debounce edits at 500ms minimum
- Append "..." while streaming, remove on final edit
- Handle Telegram's 4096-char limit (send continuation message if needed)

**Dependencies:**
- anthropic-sdk-rust streaming support (check if exists, else poll-based approach)
- teloxide `edit_message_text`

**Debouncing logic:**
```rust
let mut last_edit = Instant::now();
let min_interval = Duration::from_millis(500);

if last_edit.elapsed() >= min_interval {
    bot.edit_message_text(chat_id, msg_id, &partial_text).await?;
    last_edit = Instant::now();
}
```

### 2.2 Thread Support

**What:** Preserve reply context in Telegram threads

**Implementation:**
- Extract `reply_to_message_id` from incoming message
- Store in conversation context
- Pass to `send_message()` as reply parameter
- Enables multi-topic discussions without confusion

**Effort:** Simple teloxide API call

### 2.3 Enhanced Error Reporting

**What:** Structured error responses with suggested actions

**Current:** "Error: File not found"
**Enhanced:**
```
Unable to read file: notes/missing.md

Possible causes:
- File doesn't exist in vault
- Path should be relative to vault root

Try:
- Use /poke to see available tools
- List directory first to find correct path
```

**Implementation:**
- Modify tool execution to return structured errors
- Add error formatters in `src/tools/mod.rs`
- Map common errors to helpful messages

### Testing Phase 2

**Telegram Test Scenarios:**

```markdown
## Streaming Responses
1. Long query (200+ tokens) → Message updates multiple times
2. Short query (<50 tokens) → Message updates once or twice
3. Query with tool use → See "Using tool X..." in stream
4. Very long response (>4096 chars) → Splits into multiple messages
5. Network interruption → Graceful fallback to non-streaming

## Thread Support
1. Reply to Lu's message → Lu's response also replies to same thread
2. Multiple threads → Each maintains separate context
3. Group chat with topics → Thread preservation works

## Enhanced Error Reporting
1. Request non-existent file → Helpful error with suggestions
2. MCP connection failure → Clear message with reconnection steps
3. Claude API rate limit → Shows retry timing
4. Invalid tool parameters → Shows expected format
```

**Exit Criteria:**
- [ ] Streaming feels responsive (updates every 500ms)
- [ ] No Telegram rate limit errors from streaming
- [ ] Thread context preserved across messages
- [ ] Error messages are actionable (users know what to do)
- [ ] Version 0.7.0 released and deployed
- [ ] Phase 1 & 2 features all work

## Phase 3: Semantic Memory

**Goal:** Intelligent, citation-backed vault search

**Features:**

| Feature | Impact | Effort | Files |
|---------|--------|--------|-------|
| Semantic Memory Search | HIGH | MEDIUM-HIGH | New: `src/mcp/memory/` (Mac) |
| Temporal Decay | LOW | LOW | `src/mcp/memory/search.py` |
| Session-Scoped Search | LOW | LOW | `src/mcp/memory/search.py` |

### 3.1 Semantic Memory Search (Hybrid Architecture)

**Mac MCP Server Components:**

**New files:**
- `src/mcp/memory/__init__.py` - Manager orchestration
- `src/mcp/memory/embeddings.py` - OpenAI API integration
- `src/mcp/memory/indexer.py` - Chunk files, generate embeddings
- `src/mcp/memory/search.py` - Hybrid search (vector + keyword)
- `src/mcp/memory/db.py` - SQLite with vec extension

**Database schema:**
```sql
CREATE TABLE chunks (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    content TEXT NOT NULL,
    embedding BLOB,  -- Vector embedding
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_file_path ON chunks(file_path);
CREATE VIRTUAL TABLE chunks_fts USING fts5(content);
```

**MCP Tools:**
```python
# memory_search(query, limit=10, min_score=0.6, scope="vault")
# memory_index()  # Rebuild index from vault
```

**Search Algorithm:**
1. Generate query embedding (OpenAI API)
2. Vector search: Find top 20 by cosine similarity
3. Keyword search: Find top 20 by FTS5 rank
4. Merge results: `0.7 * vector_score + 0.3 * keyword_score`
5. Apply temporal decay: `score * e^(-age_days / 30)`
6. Filter by min_score threshold
7. Return top N with citations

**Citation Format:**
```
[[notes/project.md#L45-L48]] - "...snippet text..."
Score: 0.87 (2 days ago)
```

### 3.2 Temporal Decay

**Formula:** `adjusted_score = base_score * e^(-age_in_days / half_life)`
**Default half-life:** 30 days
**Effect:** 30-day-old memory scores 60% of fresh memory

**Implementation:** Single function in `search.py`, applied after hybrid scoring

### 3.3 Session-Scoped Search

**What:** Filter results by conversation scope

**Scopes:**
- `"vault"` - Search entire vault (default)
- `"session"` - Search only current conversation's mentioned files/topics
- `"recent"` - Search only files modified in last 7 days

**Implementation:** Add `scope` parameter to memory_search tool, filter results accordingly

### Testing Phase 3

**Telegram Test Scenarios:**

```markdown
## Semantic Memory - Basic Functionality
1. Ask: "What do I have about Claude API?"
   - [ ] Returns relevant files (not just exact keyword matches)
   - [ ] Shows citations with file:line
   - [ ] Snippets are coherent

2. Ask: "Notes on machine learning"
   - [ ] Finds "AI", "neural networks", related concepts
   - [ ] Scores make sense (most relevant first)

3. Ask about obscure topic you know exists
   - [ ] Finds it even with different wording
   - [ ] Citation navigates to correct location

## Temporal Decay
1. Create new note about "test topic"
2. Search for "test topic"
   - [ ] New note ranks higher than old notes on same topic
   - [ ] Age shown in results

3. Search for old topic (from years ago)
   - [ ] Still findable but lower ranking
   - [ ] Recent mentions rank higher

## Session-Scoped Search
1. Discuss a specific project in conversation
2. Ask: "What did we just talk about?"
   - [ ] Finds conversation context (session scope)
   - [ ] Doesn't return unrelated vault content

3. Ask: "What's in my vault about [same topic]?"
   - [ ] Vault-wide search (includes everything)

## Performance & Scale
1. Large vault (10k+ files)
   - [ ] Search completes in <3 seconds
   - [ ] Results are relevant (not just speed)

2. Index rebuild
   - [ ] Completes without errors
   - [ ] Progress indication (if possible)

3. Concurrent searches
   - [ ] Multiple users/queries don't interfere
   - [ ] No database locks

## Citation Accuracy
1. Click/copy citation → Open in Obsidian
   - [ ] Line numbers match snippet
   - [ ] Multi-line ranges are accurate

2. Verify source attribution
   - [ ] Every result shows clear source
   - [ ] Can trace back to original file
```

**Exit Criteria:**
- [ ] Semantic search returns relevant results (not just keyword matches)
- [ ] Citations are accurate (file:line matches content)
- [ ] Temporal decay visibly affects ranking
- [ ] Performance acceptable (<3s searches)
- [ ] All Phase 1 and 2 features still work
- [ ] Version 0.8.0 released and deployed

## Architecture: Semantic Memory (Hybrid)

**Why Hybrid:**
- Mac has vault files and compute for embeddings
- Pi just needs to call search API
- Clean separation: Pi = UX, Mac = intelligence
- Aligns with existing MCP architecture

**Flow:**

```
User (Telegram)
    ↓
Pi Ludolph Bot
    ↓ (calls memory_search MCP tool)
Mac MCP Server
    ├─ SQLite + vec extension
    ├─ Embedding provider (OpenAI API)
    ├─ Hybrid search (vector + keyword)
    └─ Return results with citations
    ↑
Pi Ludolph Bot
    ↓ (formats results)
User (Telegram)
```

## Files to Create/Modify

### Phase 1
- Modify: `src/bot.rs` (reactions, typing, registration)
- Modify: `src/setup.rs` (finish polish)

### Phase 2
- Modify: `src/claude.rs` (streaming support)
- Modify: `src/bot.rs` (message editing, thread support)
- Modify: `src/tools/mod.rs` (error formatting)

### Phase 3
- New: `src/mcp/memory/__init__.py`
- New: `src/mcp/memory/embeddings.py`
- New: `src/mcp/memory/indexer.py`
- New: `src/mcp/memory/search.py`
- New: `src/mcp/memory/db.py`
- Modify: `src/mcp/tools/__init__.py` (register memory tools)

## Design Decisions

| Question | Decision | Rationale |
|----------|----------|-----------|
| Separate plans per phase? | Yes | Focused scope, clear testing gates |
| Where to run semantic search? | Hybrid (Mac MCP + Pi bot) | Mac has vault + compute, Pi has UX |
| Embedding provider? | OpenAI (primary), local fallback | Battle-tested, good quality, fallback for offline |
| Testing approach? | Manual Telegram + automated cargo test | Both needed for UX validation |
| Version bumps? | Minor per phase (0.6, 0.7, 0.8) | Phases are substantial improvements |

## Additional Consideration: Multi-Provider Support

**User request:** Decouple AI service to support Claude, Gemini, OpenAI, Llama, etc.

**Impact:** HIGH - Flexibility, fallback options, cost optimization
**Effort:** MEDIUM
**Best timing:** After Phase 1 or 2 (before Phase 3, since embeddings need provider too)

**Implementation approach:**
- Create provider trait/interface
- Implement for each provider (Claude, Gemini, OpenAI, local Llama)
- Configuration: `model_provider = "claude"` in config.toml
- Fallback chain: Try primary, fall back to secondary

**Files:**
- New: `src/providers/mod.rs` (trait)
- New: `src/providers/claude.rs` (current implementation)
- New: `src/providers/gemini.rs`
- New: `src/providers/openai.rs`
- New: `src/providers/local.rs` (Llama via ollama)
- Modify: `src/claude.rs` → `src/chat.rs` (rename, generalize)

**Should this be:**
- Phase 1.5 (before streaming)?
- Phase 2.5 (after UX improvements)?
- Phase 4 (separate track)?

We can decide after seeing Phase 1-3 designs.

## Success Criteria

**Overall:**
- All 10 features implemented and tested
- Each phase delivered as working release
- No regressions between phases
- User testing validates UX improvements
- Documentation updated for new features

**Per Phase:**
- Exit criteria met before next phase
- Explicit user approval to proceed
- Release deployed and verified on Pi
