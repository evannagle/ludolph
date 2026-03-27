# Vault Index Design

Date: 2026-03-26
Branch: feature/evaluate-candlekeeps-bookreading-approach
Status: Approved

## Problem

Lu currently has raw file access to the vault — search by text, read files, list directories. For a large vault (7,749 files, 14MB), this means cold-searching on every query with no pre-existing understanding of what the vault contains, how it's structured, or what concepts it covers. Lu can *find* files but doesn't *understand* the vault.

## Solution

A tiered vault indexing system that chunks and optionally enriches vault content, giving Lu a persistent comprehension layer it can draw on when answering questions.

## Design Decisions

- **Claude-only enrichment, no vector DB** — The comprehension value comes from Claude-generated summaries, not embeddings. No new infrastructure dependencies. Vector search can be layered on later.
- **Plain file storage** — Chunks stored as JSON files in `.ludolph/index/` inside the vault. Invisible to Obsidian (dotfolder), accessible to Lu's existing file tools.
- **Three tiers** — Quick (file map), Standard (chunked), Deep (chunked + enriched). Users choose during setup based on cost/value tradeoff.
- **File watcher in bot process** — Not a separate daemon. Keeps deployment simple (one binary, one process).

## Storage Layout

The index lives inside the vault at `<vault>/.ludolph/index/`. This is invisible to Obsidian (which ignores dotfolders by default) and co-locates the index with its source data — the index survives vault moves and doesn't require a separate path mapping.

```
<vault>/
  .ludolph/
    index/
      manifest.json
      chunks/
        notes/
          meeting-2026-03.json
          chord-theory.json
        daily/
          2026-03-26.json
        ...mirrors vault folder structure...
```

### manifest.json

```json
{
  "vault_path": "/Users/evannagle/Vaults/Noggin",
  "tier": "standard",
  "file_count": 7749,
  "chunk_count": 22341,
  "last_indexed": "2026-03-26T10:00:00Z",
  "version": 1,
  "folders": {
    "notes": { "file_count": 342, "chunk_count": 1205 },
    "daily": { "file_count": 365, "chunk_count": 730 },
    "projects": { "file_count": 89, "chunk_count": 456 }
  }
}
```

The `folders` field provides a top-level directory breakdown with file and chunk counts, computed during indexing. This powers the `vault_map()` tool without requiring a runtime directory scan.

### Chunk File Format

One JSON file per source note, mirroring vault folder structure:

```json
{
  "source": "notes/chord-theory.md",
  "source_hash": "abc123def456",
  "indexed_at": "2026-03-26T10:00:00Z",
  "tier": "deep",
  "frontmatter": {
    "title": "Chord Theory",
    "tags": ["music", "theory"]
  },
  "chunks": [
    {
      "id": "chord-theory-0",
      "heading_path": ["Chord Theory", "Tritone Substitutions"],
      "content": "raw chunk text...",
      "summary": "Explains tritone substitution as replacing a dominant chord with one a tritone away...",
      "char_count": 487,
      "position": 0
    }
  ]
}
```

- `source_hash` enables cheap staleness detection (xxhash of file contents)
- `tier` tracks what processing level was actually applied (may differ from configured tier if enrichment failed)
- `id` is `{file_stem}-{position_index}` (e.g., `chord-theory-0`, `chord-theory-1`)
- Quick tier: manifest only, no chunk files
- Standard tier: chunks with content, no summary
- Deep tier: chunks with content + Claude-generated summary

### Edge Cases

- **Files with no headings** — the entire body (after frontmatter) becomes a single chunk with an empty `heading_path`. Size guard still applies if the body exceeds 1000 chars.
- **Frontmatter-only / empty-body files** — indexed in the manifest file count but produce zero chunks. No chunk file is written.
- **Non-markdown files** — only `.md` files are indexed, consistent with the existing search tool. All other file types are skipped.
- **Very large files** (>100KB) — chunked normally but capped at 200 chunks per file. Files exceeding this are partially indexed with a warning logged.
- **Filename collisions** — if both `notes/foo.md` and `notes/foo/bar.md` exist, the chunk file for `foo.md` is written as `notes/foo.json` and the directory as `notes/foo/`. JSON files and directories can coexist at the filesystem level.

## Chunking Pipeline

Each markdown file is processed through these steps:

1. **Parse frontmatter** — extract YAML metadata (title, tags, aliases, dates). Store as file-level metadata.

2. **Header-aware splitting** — split on markdown headings (`#` through `######`). Each heading + its content becomes a chunk. Preserves the heading path (e.g., `["Chord Theory", "Tritone Substitutions"]`).

3. **Size guard** — sections exceeding 1000 characters are split further on paragraph boundaries. Single paragraphs exceeding 1000 characters are hard-split with 100-character overlap.

4. **Small chunk merging** — sections under 200 characters are merged with their next sibling to avoid tiny fragments.

5. **Metadata attachment** — each chunk carries: source file path, heading path, position index, character count, frontmatter tags.

6. **Wikilink preservation** — `[[wikilinks]]` are preserved as-is so Lu can follow connections between notes.

### Deep Tier Enrichment

Each chunk is sent to Claude Haiku with:

> "Summarize this chunk in 1-2 sentences. What concept does it capture? What would someone search for to find this?"

The response becomes the `summary` field. Batched in groups of 10-20 chunks for throughput.

**Enrichment failure handling:** if a batch fails (API error, rate limit, timeout), the affected chunks are saved without summaries (effectively Standard tier). The chunk file's `tier` field is set to `"standard"` to reflect what was actually applied. Failed chunks are logged and retried on the next `lu index` run (they'll be detected as "Deep tier configured but chunk is Standard tier").

## CLI: `lu index`

```
lu index                    # index at configured tier
lu index --tier quick       # file map only
lu index --tier standard    # chunk without enrichment
lu index --tier deep        # chunk + Claude enrichment
lu index --rebuild          # full rebuild, ignore existing index
lu index --status           # show index health
```

### Behavior

- **Incremental by default** — compares `source_hash` per file, only re-processes changed/new/deleted files.
- **Progress output** — Pi spinner with counter: `[31415] Indexing... 142/7749 files`
- **Cost confirmation** — Deep tier on large vaults (>100 files) shows estimated cost and prompts: `Deep indexing ~7749 files. Estimated cost: ~$18. Continue? [y/n]`
- **Resumable** — interrupted runs pick up where they left off (already-indexed files have hash recorded).
- **Streaming writes** — files are processed and written one at a time, not accumulated in memory. Safe for Pi's limited RAM.
- **Lock file** — writes `<vault>/.ludolph/index/.lock` while indexing. If the bot's file watcher detects the lock, it queues changes instead of writing. If `lu index` detects the lock, it exits with an error: "Index is locked by another process."
- **Exit codes** — 0 success, 1 partial failure (logs which files failed).

## Setup Integration

After vault path configuration in `lu setup`:

```
Vault found: 7,749 files (14MB)

How should Lu learn your vault?

  1. Quick    — file map only (free, ~5 seconds)
  2. Standard — chunked index (free, ~2 minutes)
  3. Deep     — chunked + AI summaries (~$18, ~3 hours)

  Choose [1/2/3]:
```

- Cost/time estimates calculated from actual vault size.
- Selection stored in `config.toml` as `index_tier`.
- Index runs immediately. Deep tier runs in background — setup completes, bot starts, user gets Telegram notification when done.

### Config Addition

```toml
[index]
tier = "standard"
```

Index path is always `<vault>/.ludolph/index/`, derived from the vault path in config. Not a separate config value.

## File Watcher

Runs inside the bot process using the `notify` crate.

### Behavior

- Watches vault directory recursively for creates, modifications, deletes.
- **5-second debounce window** — batches rapid changes (Obsidian autosave, Git pulls).
- Processes batch: re-chunk changed files, remove entries for deleted files, add entries for new files.
- Operates at configured tier — Standard watcher chunks only, Deep watcher also enriches.
- **Non-blocking** — background tokio task, never interferes with message handling.
- Logs at debug level: `Re-indexed notes/chord-theory.md (3 chunks)`

### Exclusions

- `.ludolph/` directory (don't index the index)
- `.obsidian/` directory
- `.trash/` directory
- Hidden files and folders
- Binary files (images, PDFs, etc.)

### Manifest Updates

After each batch, manifest file count and last-indexed timestamp are updated.

## How Lu Uses the Index

### New Tools

Two new Claude tools supplement the existing `read_file`, `search`, and `list_directory`:

**`search_index(query: str, max_results: int)`**
Searches chunk content and summaries (when available). Returns top N matching chunks with heading paths, source files, and summaries.

Search algorithm: regex-based matching (same engine as the existing `search` tool) against both chunk `content` and `summary` fields. Chunks are ranked by: (1) summary match scores higher than content-only match, (2) shorter chunks with matches score higher (higher signal density), (3) position 0 chunks (file intro) get a small boost. Returns chunks sorted by score with source file, heading path, and summary (if available).

For Deep tier, searching against summaries is the key advantage — a search for "how grief functions in narrative" matches chunks whose summary mentions "loss as a narrative device" even if the raw text doesn't contain the word "grief."

**Tier-specific behavior:** Quick tier has no chunks — `search_index` returns an empty result with a message: "Index is at Quick tier (file map only). Run `lu index --tier standard` for chunk search." Claude falls back to the existing `search` tool.

**`vault_map()`**
Returns the manifest contents: vault path, tier, file/chunk counts, last-indexed time, and the `folders` breakdown with per-folder file and chunk counts. Gives Claude a bird's-eye view before diving into specific files.

### Query Flow

1. User asks question via Telegram
2. Lu loads manifest — Claude knows vault structure upfront
3. Lu searches chunk index — matching chunks included as context
4. Claude still has raw tools as fallback for drilling into specific files
5. Answers grounded in pre-processed understanding

## Cost Estimates

Based on a 7,749-file / 14MB vault:

| Tier | API Cost | Time | Storage |
|------|----------|------|---------|
| Quick | $0 | ~5s | ~100KB (manifest only) |
| Standard | $0 | ~2min | ~20MB (chunks) |
| Deep | ~$15-25 (Haiku) | ~3hrs | ~25MB (chunks + summaries) |

Incremental updates (file watcher) are negligible cost — a few chunks per file change.

## Dependencies

| Crate | Purpose | Notes |
|-------|---------|-------|
| `notify` | File watching | Cross-platform, well-established |
| `serde_json` | Chunk serialization | Already a dependency |
| `sha2` or `xxhash` | File hashing for staleness | Lightweight |
| `pulldown-cmark` | Markdown heading parsing | Standard Rust markdown parser |

No new heavy dependencies. No vector database. No embedding model.

### Deployment Notes

- **inotify watch limit on Pi:** the `notify` crate uses inotify on Linux. The default per-user watch limit (8192) may be insufficient for large vaults with many subdirectories. If exceeded, `notify` falls back to polling mode automatically. For optimal performance on Pi, users may need to increase `fs.inotify.max_user_watches` via sysctl.
- **Index loading:** at bot startup, the manifest is loaded into memory for fast `vault_map()` responses. Chunk files are read on-demand during `search_index` calls (not pre-loaded), relying on OS file cache for performance.

## Testing Strategy

### Unit Tests

- Chunking: header splitting, size guards, small chunk merging, frontmatter extraction
- Staleness detection: hash comparison, incremental indexing decisions
- Wikilink preservation through chunking
- Tier behavior: Quick produces manifest only, Standard produces chunks, Deep adds summaries
- Exclusion rules: hidden files, binary files, index directory

### Integration Tests

- `lu index` on a fixture vault
- Incremental re-index after file changes
- File watcher debouncing and batch processing
- Setup wizard tier selection and config persistence
- `search_index` and `vault_map` tool responses

### Release Verification

- All existing tests still pass
- New tests pass
- `cargo clippy` and `cargo fmt` clean
- `lu doctor` checks pass
- Build for Mac and Pi targets
- Deploy to Pi, run `lu index` on real vault
- Verify Lu uses index context in Telegram responses

## Out of Scope

- Vector/embedding search (future tier upgrade)
- `lu learn` for external sources
- `lu teach` / knowledge packaging
- Marketplace / sharing site
- These are tracked in noggin task me-0326b

## Future Considerations

The storage format is designed to accommodate embeddings later — a future `embeddings` field on each chunk would enable vector search without restructuring. The tiering system can grow (e.g., a "Semantic" tier above Deep that adds Voyage embeddings) without breaking existing indexes.
