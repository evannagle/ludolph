# Vault Index Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Lu a persistent comprehension layer over the vault via tiered indexing (Quick/Standard/Deep) with chunking, optional Claude enrichment, file watching, and two new search tools.

**Architecture:** Markdown files are chunked (header-aware splitting), optionally enriched with Claude summaries, and stored as JSON in `<vault>/.ludolph/index/`. A file watcher in the bot process keeps the index fresh. Two new Claude tools (`search_index`, `vault_map`) let Lu query the index during conversations.

**Tech Stack:** Rust 2024, pulldown-cmark (markdown parsing), notify (file watching), xxhash (file hashing), serde_json (chunk storage). Claude Haiku for Deep tier enrichment via existing reqwest/API client.

**Spec:** `docs/superpowers/specs/2026-03-26-vault-index-design.md`

---

## File Structure

### New Files

| File | Responsibility |
|------|---------------|
| `src/index/mod.rs` | Module root. Exports `IndexConfig`, `Manifest`, `Indexer`, `Watcher`. Re-exports submodules. |
| `src/index/manifest.rs` | `Manifest` struct (load/save/update). Folder stats computation. Lock file management. |
| `src/index/chunker.rs` | Chunking pipeline: frontmatter parsing, header-aware splitting, size guards, small chunk merging. |
| `src/index/enricher.rs` | Deep tier enrichment: batch chunks to Claude Haiku, handle failures gracefully. |
| `src/index/indexer.rs` | `Indexer` orchestrator: walks vault, detects staleness, coordinates chunking/enrichment, writes chunk files. |
| `src/index/watcher.rs` | File watcher using `notify` crate. Debouncing, exclusion rules, triggers re-indexing. |
| `src/tools/search_index.rs` | `search_index` Claude tool: searches chunk content/summaries, ranked results. |
| `src/tools/vault_map.rs` | `vault_map` Claude tool: returns manifest data for vault overview. |

### Modified Files

| File | Changes |
|------|---------|
| `src/config.rs` | Add `IndexConfig` struct with `tier` field. Add `index` field to `Config`. |
| `src/cli/mod.rs` | Add `Index` variant to `Command` enum with `--tier`, `--rebuild`, `--status` flags. |
| `src/main.rs` | Add match arm for `Command::Index` routing to handler. |
| `src/cli/setup/mod.rs` | Add index tier selection step after vault path configuration. |
| `src/tools/mod.rs` | Register `search_index` and `vault_map` tools. |
| `src/bot.rs` | Spawn file watcher on startup. |
| `Cargo.toml` | Add `pulldown-cmark`, `notify-debouncer-mini`, `xxhash-rust` dependencies. |

---

## Task 1: Add Dependencies and IndexConfig

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/config.rs`
- Create: `src/index/mod.rs`

- [ ] **Step 1: Add new dependencies to Cargo.toml**

Add under `[dependencies]`:
```toml
pulldown-cmark = "0.12"
notify-debouncer-mini = "0.5"
xxhash-rust = { version = "0.8", features = ["xxh3"] }
```

- [ ] **Step 2: Define IndexConfig in config.rs**

Add after the existing `SchedulerConfig` struct:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IndexTier {
    Quick,
    Standard,
    Deep,
}

impl Default for IndexTier {
    fn default() -> Self {
        Self::Standard
    }
}

impl std::fmt::Display for IndexTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Quick => write!(f, "quick"),
            Self::Standard => write!(f, "standard"),
            Self::Deep => write!(f, "deep"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    #[serde(default)]
    pub tier: IndexTier,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            tier: IndexTier::default(),
        }
    }
}
```

- [ ] **Step 3: Add index field to Config struct**

Add to the `Config` struct:
```rust
#[serde(default)]
pub index: IndexConfig,
```

And to `Config::new()`:
```rust
index: IndexConfig::default(),
```

- [ ] **Step 4: Create src/index/mod.rs**

```rust
pub mod manifest;
pub mod chunker;
pub mod indexer;
pub mod enricher;
pub mod watcher;

pub use manifest::Manifest;
pub use chunker::{chunk_markdown, Chunk};
pub use indexer::Indexer;
```

- [ ] **Step 5: Add `mod index;` to main.rs**

Add `mod index;` alongside other module declarations at the top of `src/main.rs`.

- [ ] **Step 6: Run cargo check**

Run: `cargo check`
Expected: Compiles (empty submodules will need stub files)

- [ ] **Step 7: Create stub files for submodules**

Create minimal stub files for each submodule so the project compiles:
- `src/index/manifest.rs` — empty
- `src/index/chunker.rs` — empty
- `src/index/indexer.rs` — empty
- `src/index/enricher.rs` — empty
- `src/index/watcher.rs` — empty

- [ ] **Step 8: Run cargo check and cargo test**

Run: `cargo check && cargo test`
Expected: Compiles, all existing tests pass.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/config.rs src/index/ src/main.rs
git commit -m "feat: add IndexConfig and index module scaffolding"
```

---

## Task 2: Chunking Pipeline

**Files:**
- Create: `src/index/chunker.rs`

- [ ] **Step 1: Write failing test for frontmatter extraction**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_frontmatter_from_markdown() {
        let input = "---\ntitle: Test Note\ntags: [music, theory]\n---\n\n# Heading\n\nBody text.";
        let result = parse_frontmatter(input);
        assert_eq!(result.metadata.get("title").unwrap(), "Test Note");
        assert!(result.body.contains("# Heading"));
        assert!(!result.body.contains("---"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test extracts_frontmatter`
Expected: FAIL — function not defined.

- [ ] **Step 3: Implement frontmatter parsing**

```rust
use serde_json::Value;
use std::collections::HashMap;

pub struct ParsedDocument {
    pub metadata: HashMap<String, String>,
    pub body: String,
}

pub fn parse_frontmatter(content: &str) -> ParsedDocument {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return ParsedDocument {
            metadata: HashMap::new(),
            body: content.to_string(),
        };
    }

    let after_first = &trimmed[3..];
    if let Some(end) = after_first.find("\n---") {
        let yaml_block = &after_first[..end];
        let body = after_first[end + 4..].trim_start().to_string();

        let mut metadata = HashMap::new();
        for line in yaml_block.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                if !key.is_empty() {
                    metadata.insert(key, value);
                }
            }
        }

        ParsedDocument { metadata, body }
    } else {
        ParsedDocument {
            metadata: HashMap::new(),
            body: content.to_string(),
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test extracts_frontmatter`
Expected: PASS

- [ ] **Step 5: Write failing test for header-aware splitting**

```rust
#[test]
fn splits_on_headings() {
    let input = "# Title\n\nIntro text.\n\n## Section A\n\nContent A.\n\n## Section B\n\nContent B.";
    let chunks = chunk_markdown(input, "test.md");
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].heading_path, vec!["Title"]);
    assert!(chunks[0].content.contains("Intro text"));
    assert_eq!(chunks[1].heading_path, vec!["Title", "Section A"]);
    assert!(chunks[1].content.contains("Content A"));
    assert_eq!(chunks[2].heading_path, vec!["Title", "Section B"]);
    assert!(chunks[2].content.contains("Content B"));
}
```

- [ ] **Step 6: Run test to verify it fails**

Run: `cargo test splits_on_headings`
Expected: FAIL

- [ ] **Step 7: Implement Chunk struct and header-aware splitting**

```rust
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: String,
    pub heading_path: Vec<String>,
    pub content: String,
    pub char_count: usize,
    pub position: usize,
}

pub fn chunk_markdown(content: &str, file_stem: &str) -> Vec<Chunk> {
    let parsed = parse_frontmatter(content);
    let body = &parsed.body;

    if body.trim().is_empty() {
        return Vec::new();
    }

    let mut sections: Vec<(Vec<String>, String)> = Vec::new();
    let mut current_heading_path: Vec<String> = Vec::new();
    let mut current_text = String::new();
    let mut in_heading = false;
    let mut heading_level: usize = 0;
    let mut heading_text = String::new();

    let parser = Parser::new(body);

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                // Save current section before starting new heading
                if !current_text.trim().is_empty() || !sections.is_empty() {
                    if !current_text.trim().is_empty() {
                        sections.push((current_heading_path.clone(), current_text.trim().to_string()));
                    }
                    current_text = String::new();
                }
                in_heading = true;
                heading_level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                // Trim heading path to current level
                current_heading_path.truncate(heading_level - 1);
                current_heading_path.push(heading_text.trim().to_string());
            }
            Event::Text(text) => {
                if in_heading {
                    heading_text.push_str(&text);
                } else {
                    current_text.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if !in_heading {
                    current_text.push('\n');
                }
            }
            Event::Code(code) => {
                if in_heading {
                    heading_text.push_str(&code);
                } else {
                    current_text.push('`');
                    current_text.push_str(&code);
                    current_text.push('`');
                }
            }
            _ => {}
        }
    }

    // Don't forget the last section
    if !current_text.trim().is_empty() {
        sections.push((current_heading_path.clone(), current_text.trim().to_string()));
    }

    // Apply size guards and merging, then build chunks
    let sections = apply_size_guards(sections);
    let sections = merge_small_sections(sections);

    sections
        .into_iter()
        .enumerate()
        .map(|(i, (path, text))| {
            let char_count = text.len();
            Chunk {
                id: format!("{file_stem}-{i}"),
                heading_path: path,
                content: text,
                char_count,
                position: i,
            }
        })
        .collect()
}

const MAX_CHUNK_SIZE: usize = 1000;
const MIN_CHUNK_SIZE: usize = 200;
const OVERLAP_SIZE: usize = 100;

fn apply_size_guards(sections: Vec<(Vec<String>, String)>) -> Vec<(Vec<String>, String)> {
    let mut result = Vec::new();
    for (path, text) in sections {
        if text.len() <= MAX_CHUNK_SIZE {
            result.push((path, text));
        } else {
            // Split on paragraph boundaries
            let paragraphs: Vec<&str> = text.split("\n\n").collect();
            let mut current = String::new();
            for para in paragraphs {
                if current.len() + para.len() > MAX_CHUNK_SIZE && !current.is_empty() {
                    result.push((path.clone(), current.trim().to_string()));
                    // Add overlap from end of previous chunk
                    let overlap_start = current.len().saturating_sub(OVERLAP_SIZE);
                    current = current[overlap_start..].to_string();
                    current.push_str("\n\n");
                }
                current.push_str(para);
                current.push_str("\n\n");
            }
            if !current.trim().is_empty() {
                result.push((path.clone(), current.trim().to_string()));
            }
        }
    }
    result
}

fn merge_small_sections(sections: Vec<(Vec<String>, String)>) -> Vec<(Vec<String>, String)> {
    if sections.is_empty() {
        return sections;
    }

    let mut result: Vec<(Vec<String>, String)> = Vec::new();
    for (path, text) in sections {
        if let Some(last) = result.last_mut() {
            if last.1.len() < MIN_CHUNK_SIZE {
                last.1.push_str("\n\n");
                last.1.push_str(&text);
                continue;
            }
        }
        result.push((path, text));
    }
    result
}
```

- [ ] **Step 8: Run test to verify it passes**

Run: `cargo test splits_on_headings`
Expected: PASS

- [ ] **Step 9: Write and run additional chunking tests**

Add tests for:
- `no_headings_becomes_single_chunk` — file with no headings produces one chunk with empty heading_path
- `empty_body_produces_no_chunks` — frontmatter-only file returns empty vec
- `size_guard_splits_large_sections` — section >1000 chars gets split
- `small_sections_are_merged` — sections <200 chars get merged with next
- `wikilinks_preserved` — `[[links]]` survive chunking intact
- `frontmatter_not_in_chunks` — YAML frontmatter excluded from chunk content

Run: `cargo test -p ludolph chunker`
Expected: All pass.

- [ ] **Step 10: Run full test suite**

Run: `cargo test`
Expected: All tests pass including existing ones.

- [ ] **Step 11: Commit**

```bash
git add src/index/chunker.rs
git commit -m "feat: header-aware markdown chunking pipeline"
```

---

## Task 3: Manifest Management

**Files:**
- Create: `src/index/manifest.rs`

- [ ] **Step 1: Write failing test for manifest creation**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_and_loads_manifest() {
        let tmp = TempDir::new().unwrap();
        let index_dir = tmp.path().join(".ludolph").join("index");

        let manifest = Manifest::new(tmp.path(), IndexTier::Standard);
        manifest.save(&index_dir).unwrap();

        let loaded = Manifest::load(&index_dir).unwrap();
        assert_eq!(loaded.tier, IndexTier::Standard);
        assert_eq!(loaded.file_count, 0);
        assert_eq!(loaded.chunk_count, 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test creates_and_loads_manifest`
Expected: FAIL

- [ ] **Step 3: Implement Manifest struct**

```rust
use crate::config::IndexTier;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderStats {
    pub file_count: usize,
    pub chunk_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub vault_path: PathBuf,
    pub tier: IndexTier,
    pub file_count: usize,
    pub chunk_count: usize,
    pub last_indexed: String,
    pub version: u32,
    #[serde(default)]
    pub folders: HashMap<String, FolderStats>,
}

impl Manifest {
    pub fn new(vault_path: &Path, tier: IndexTier) -> Self {
        Self {
            vault_path: vault_path.to_path_buf(),
            tier,
            file_count: 0,
            chunk_count: 0,
            last_indexed: chrono::Utc::now().to_rfc3339(),
            version: 1,
            folders: HashMap::new(),
        }
    }

    pub fn save(&self, index_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(index_dir)?;
        let path = index_dir.join("manifest.json");
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    pub fn load(index_dir: &Path) -> Result<Self> {
        let path = index_dir.join("manifest.json");
        let contents = std::fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&contents)?;
        Ok(manifest)
    }

    pub fn index_dir(vault_path: &Path) -> PathBuf {
        vault_path.join(".ludolph").join("index")
    }

    pub fn chunks_dir(vault_path: &Path) -> PathBuf {
        Self::index_dir(vault_path).join("chunks")
    }

    pub fn lock_path(vault_path: &Path) -> PathBuf {
        Self::index_dir(vault_path).join(".lock")
    }

    pub fn acquire_lock(vault_path: &Path) -> Result<LockGuard> {
        let lock_path = Self::lock_path(vault_path);
        if lock_path.exists() {
            anyhow::bail!("Index is locked by another process. Remove {} if stale.", lock_path.display());
        }
        std::fs::write(&lock_path, std::process::id().to_string())?;
        Ok(LockGuard { path: lock_path })
    }
}

pub struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test creates_and_loads_manifest`
Expected: PASS

- [ ] **Step 5: Write and run tests for lock file**

```rust
#[test]
fn lock_prevents_concurrent_access() {
    let tmp = TempDir::new().unwrap();
    let index_dir = Manifest::index_dir(tmp.path());
    std::fs::create_dir_all(&index_dir).unwrap();

    let _guard = Manifest::acquire_lock(tmp.path()).unwrap();
    let second = Manifest::acquire_lock(tmp.path());
    assert!(second.is_err());
}

#[test]
fn lock_released_on_drop() {
    let tmp = TempDir::new().unwrap();
    let index_dir = Manifest::index_dir(tmp.path());
    std::fs::create_dir_all(&index_dir).unwrap();

    {
        let _guard = Manifest::acquire_lock(tmp.path()).unwrap();
    }
    // Lock should be released
    let _guard = Manifest::acquire_lock(tmp.path()).unwrap();
}
```

Run: `cargo test lock_`
Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/index/manifest.rs
git commit -m "feat: manifest management with lock file support"
```

---

## Task 4: Indexer Orchestrator

**Files:**
- Create: `src/index/indexer.rs`

- [ ] **Step 1: Write failing test for single-file indexing**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::IndexTier;
    use tempfile::TempDir;

    fn create_test_vault(dir: &Path) {
        let notes = dir.join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(
            notes.join("test.md"),
            "---\ntitle: Test\n---\n\n# Heading\n\nSome content here.",
        ).unwrap();
    }

    #[tokio::test]
    async fn indexes_single_file() {
        let tmp = TempDir::new().unwrap();
        create_test_vault(tmp.path());

        let indexer = Indexer::new(tmp.path(), IndexTier::Standard);
        let stats = indexer.run(false).await.unwrap();

        assert_eq!(stats.files_indexed, 1);
        assert!(stats.chunks_created > 0);

        // Verify chunk file exists
        let chunk_path = Manifest::chunks_dir(tmp.path()).join("notes").join("test.json");
        assert!(chunk_path.exists());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test indexes_single_file`
Expected: FAIL

- [ ] **Step 3: Implement ChunkFile struct and Indexer**

```rust
use crate::config::IndexTier;
use crate::index::chunker::{chunk_markdown, Chunk};
use crate::index::manifest::{FolderStats, Manifest};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize)]
pub struct ChunkFile {
    pub source: String,
    pub source_hash: String,
    pub indexed_at: String,
    pub tier: String,
    pub frontmatter: HashMap<String, String>,
    pub chunks: Vec<StoredChunk>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredChunk {
    pub id: String,
    pub heading_path: Vec<String>,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub char_count: usize,
    pub position: usize,
}

impl From<Chunk> for StoredChunk {
    fn from(c: Chunk) -> Self {
        Self {
            id: c.id,
            heading_path: c.heading_path,
            content: c.content,
            summary: None,
            char_count: c.char_count,
            position: c.position,
        }
    }
}

pub struct IndexStats {
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub chunks_created: usize,
    pub files_removed: usize,
}

pub struct Indexer {
    vault_path: PathBuf,
    tier: IndexTier,
}

impl Indexer {
    pub fn new(vault_path: &Path, tier: IndexTier) -> Self {
        Self {
            vault_path: vault_path.to_path_buf(),
            tier,
        }
    }

    pub async fn run(&self, rebuild: bool) -> Result<IndexStats> {
        let index_dir = Manifest::index_dir(&self.vault_path);
        let chunks_dir = Manifest::chunks_dir(&self.vault_path);
        let _lock = Manifest::acquire_lock(&self.vault_path)?;

        // Load existing manifest or create new
        let mut manifest = if rebuild {
            if chunks_dir.exists() {
                std::fs::remove_dir_all(&chunks_dir)?;
            }
            Manifest::new(&self.vault_path, self.tier.clone())
        } else {
            Manifest::load(&index_dir).unwrap_or_else(|_| {
                Manifest::new(&self.vault_path, self.tier.clone())
            })
        };

        // Quick tier: manifest only
        if self.tier == IndexTier::Quick {
            let file_count = self.count_markdown_files();
            manifest.file_count = file_count;
            manifest.tier = self.tier.clone();
            manifest.last_indexed = chrono::Utc::now().to_rfc3339();
            manifest.folders = self.compute_folder_stats_quick();
            manifest.save(&index_dir)?;
            return Ok(IndexStats {
                files_indexed: 0,
                files_skipped: file_count,
                chunks_created: 0,
                files_removed: 0,
            });
        }

        // Load existing hashes for incremental indexing
        let existing_hashes = self.load_existing_hashes(&chunks_dir);

        let mut stats = IndexStats {
            files_indexed: 0,
            files_skipped: 0,
            chunks_created: 0,
            files_removed: 0,
        };

        let mut folder_stats: HashMap<String, FolderStats> = HashMap::new();

        // Walk vault and index
        for entry in WalkDir::new(&self.vault_path)
            .into_iter()
            .filter_entry(|e| !self.should_exclude(e.path()))
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !path.is_file() || path.extension().is_some_and(|e| e != "md") {
                continue;
            }

            let relative = path.strip_prefix(&self.vault_path)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => { stats.files_skipped += 1; continue; }
            };

            let hash = self.hash_content(&content);

            // Skip if unchanged
            if let Some(existing_hash) = existing_hashes.get(&relative) {
                if *existing_hash == hash {
                    // Still count for folder stats
                    let folder = self.top_folder(&relative);
                    let entry = folder_stats.entry(folder).or_insert(FolderStats { file_count: 0, chunk_count: 0 });
                    // We'd need to read existing chunk count — simplify by just counting files
                    entry.file_count += 1;
                    stats.files_skipped += 1;
                    continue;
                }
            }

            let file_stem = path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            let parsed = crate::index::chunker::parse_frontmatter(&content);
            let chunks = chunk_markdown(&content, &file_stem);

            let chunk_file = ChunkFile {
                source: relative.clone(),
                source_hash: hash,
                indexed_at: chrono::Utc::now().to_rfc3339(),
                tier: self.tier.to_string(),
                frontmatter: parsed.metadata,
                chunks: chunks.iter().cloned().map(StoredChunk::from).collect(),
            };

            // Write chunk file
            let chunk_path = self.chunk_file_path(&chunks_dir, &relative);
            if let Some(parent) = chunk_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string_pretty(&chunk_file)?;
            std::fs::write(&chunk_path, json)?;

            let folder = self.top_folder(&relative);
            let entry = folder_stats.entry(folder).or_insert(FolderStats { file_count: 0, chunk_count: 0 });
            entry.file_count += 1;
            entry.chunk_count += chunk_file.chunks.len();

            stats.files_indexed += 1;
            stats.chunks_created += chunk_file.chunks.len();
        }

        // Clean up removed files
        stats.files_removed = self.cleanup_removed_files(&chunks_dir)?;

        // Update manifest
        manifest.file_count = stats.files_indexed + stats.files_skipped;
        manifest.chunk_count = stats.chunks_created; // TODO: include existing chunks from skipped files
        manifest.tier = self.tier.clone();
        manifest.last_indexed = chrono::Utc::now().to_rfc3339();
        manifest.folders = folder_stats;
        manifest.save(&index_dir)?;

        Ok(stats)
    }

    fn should_exclude(&self, path: &Path) -> bool {
        let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        name.starts_with('.') || name == "node_modules"
    }

    fn hash_content(&self, content: &str) -> String {
        format!("{:x}", xxhash_rust::xxh3::xxh3_64(content.as_bytes()))
    }

    fn chunk_file_path(&self, chunks_dir: &Path, relative_md: &str) -> PathBuf {
        let json_path = relative_md.replace(".md", ".json");
        chunks_dir.join(json_path)
    }

    fn top_folder(&self, relative: &str) -> String {
        relative.split('/').next().unwrap_or("root").to_string()
    }

    fn count_markdown_files(&self) -> usize {
        WalkDir::new(&self.vault_path)
            .into_iter()
            .filter_entry(|e| !self.should_exclude(e.path()))
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
            .count()
    }

    fn compute_folder_stats_quick(&self) -> HashMap<String, FolderStats> {
        let mut stats: HashMap<String, FolderStats> = HashMap::new();
        for entry in WalkDir::new(&self.vault_path)
            .into_iter()
            .filter_entry(|e| !self.should_exclude(e.path()))
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "md") {
                let relative = path.strip_prefix(&self.vault_path).unwrap_or(path);
                let folder = relative.iter().next()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "root".to_string());
                let entry = stats.entry(folder).or_insert(FolderStats { file_count: 0, chunk_count: 0 });
                entry.file_count += 1;
            }
        }
        stats
    }

    fn load_existing_hashes(&self, chunks_dir: &Path) -> HashMap<String, String> {
        let mut hashes = HashMap::new();
        if !chunks_dir.exists() {
            return hashes;
        }
        for entry in WalkDir::new(chunks_dir).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "json") {
                if let Ok(contents) = std::fs::read_to_string(path) {
                    if let Ok(chunk_file) = serde_json::from_str::<ChunkFile>(&contents) {
                        hashes.insert(chunk_file.source, chunk_file.source_hash);
                    }
                }
            }
        }
        hashes
    }

    fn cleanup_removed_files(&self, chunks_dir: &Path) -> Result<usize> {
        let mut removed = 0;
        if !chunks_dir.exists() {
            return Ok(0);
        }
        for entry in WalkDir::new(chunks_dir).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "json") {
                if let Ok(contents) = std::fs::read_to_string(path) {
                    if let Ok(chunk_file) = serde_json::from_str::<ChunkFile>(&contents) {
                        let source_path = self.vault_path.join(&chunk_file.source);
                        if !source_path.exists() {
                            std::fs::remove_file(path)?;
                            removed += 1;
                        }
                    }
                }
            }
        }
        Ok(removed)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test indexes_single_file`
Expected: PASS

- [ ] **Step 5: Write and run additional indexer tests**

Add tests for:
- `incremental_skips_unchanged_files` — same file re-indexed without changes produces skipped count
- `rebuild_clears_existing_index` — rebuild flag removes old chunks
- `quick_tier_produces_manifest_only` — no chunk files created
- `excludes_hidden_directories` — `.obsidian/` and `.ludolph/` are skipped
- `handles_empty_vault` — vault with no markdown files produces empty manifest

Run: `cargo test -p ludolph indexer`
Expected: All pass.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/index/indexer.rs
git commit -m "feat: vault indexer with incremental updates and staleness detection"
```

---

## Task 5: CLI Command `lu index`

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`
- Create or modify: `src/cli/commands.rs` (add `index_cmd` function)

- [ ] **Step 1: Add Index variant to Command enum**

In `src/cli/mod.rs`, add to the `Command` enum:
```rust
/// Build or rebuild the vault index
Index {
    /// Index tier: quick, standard, or deep
    #[arg(long)]
    tier: Option<String>,

    /// Full rebuild, ignoring existing index
    #[arg(long)]
    rebuild: bool,

    /// Show index health status
    #[arg(long)]
    status: bool,
},
```

- [ ] **Step 2: Add match arm in main.rs**

In `src/main.rs` `run()` function, add:
```rust
Some(Command::Index { tier, rebuild, status }) => {
    cli::index_cmd(tier, rebuild, status).await?;
    Ok(ExitCode::SUCCESS)
}
```

- [ ] **Step 3: Implement index_cmd handler**

In `src/cli/commands.rs` (or a new `src/cli/index.rs` if the file is large):

```rust
pub async fn index_cmd(
    tier_override: Option<String>,
    rebuild: bool,
    status: bool,
) -> Result<()> {
    let config = Config::load()?;
    let vault_path = config.vault.as_ref()
        .ok_or_else(|| anyhow::anyhow!("Vault not configured. Run `lu setup` first."))?
        .path.clone();

    let vault_path = shellexpand::tilde(&vault_path).to_string();
    let vault_path = std::path::Path::new(&vault_path);

    if !vault_path.exists() {
        anyhow::bail!("Vault path does not exist: {}", vault_path.display());
    }

    let index_dir = Manifest::index_dir(vault_path);

    if status {
        return show_index_status(&index_dir, vault_path);
    }

    let tier = match tier_override.as_deref() {
        Some("quick") => IndexTier::Quick,
        Some("standard") => IndexTier::Standard,
        Some("deep") => IndexTier::Deep,
        Some(other) => anyhow::bail!("Unknown tier: {other}. Use quick, standard, or deep."),
        None => config.index.tier.clone(),
    };

    // Cost confirmation for deep tier
    if tier == IndexTier::Deep {
        let file_count = count_vault_files(vault_path);
        if file_count > 100 {
            let est_cost = estimate_deep_cost(file_count);
            println!("  Deep indexing ~{file_count} files. Estimated cost: ~${est_cost:.0}");
            let proceed = ui::prompt::confirm("Continue?")?;
            if !proceed {
                println!("  Cancelled.");
                return Ok(());
            }
        }
    }

    let action = if rebuild { "Rebuilding" } else { "Indexing" };
    let spinner = ui::Spinner::new(&format!("{action} vault ({tier})..."));

    let indexer = Indexer::new(vault_path, tier);
    match indexer.run(rebuild).await {
        Ok(stats) => {
            spinner.finish();
            println!(
                "  {} files indexed, {} chunks created, {} skipped, {} removed",
                stats.files_indexed, stats.chunks_created,
                stats.files_skipped, stats.files_removed,
            );
        }
        Err(e) => {
            spinner.finish_error();
            anyhow::bail!("Indexing failed: {e}");
        }
    }

    Ok(())
}

fn show_index_status(index_dir: &Path, vault_path: &Path) -> Result<()> {
    match Manifest::load(index_dir) {
        Ok(manifest) => {
            ui::StatusLine::ok(format!(
                "Index: {} files, {} chunks, tier: {}, last indexed: {}",
                manifest.file_count, manifest.chunk_count,
                manifest.tier, manifest.last_indexed,
            )).print();
            if !manifest.folders.is_empty() {
                println!("  Folders:");
                for (folder, stats) in &manifest.folders {
                    println!("    {folder}: {} files, {} chunks", stats.file_count, stats.chunk_count);
                }
            }
        }
        Err(_) => {
            ui::StatusLine::skip("No index found. Run `lu index` to create one.").print();
        }
    }
    Ok(())
}

fn count_vault_files(vault_path: &Path) -> usize {
    walkdir::WalkDir::new(vault_path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
        .count()
}

fn estimate_deep_cost(file_count: usize) -> f64 {
    // ~3 chunks per file avg, ~250 input tokens per chunk, Haiku pricing
    let chunks = file_count as f64 * 3.0;
    let input_tokens = chunks * 250.0;
    let output_tokens = chunks * 50.0;
    // Haiku: $0.25/M input, $1.25/M output
    (input_tokens * 0.25 + output_tokens * 1.25) / 1_000_000.0
}
```

- [ ] **Step 4: Export index_cmd from cli module**

Add to `src/cli/mod.rs` exports.

- [ ] **Step 5: Run cargo check**

Run: `cargo check`
Expected: Compiles.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/cli/mod.rs src/main.rs src/cli/commands.rs
git commit -m "feat: lu index CLI command with tier selection and status"
```

---

## Task 6: Claude Tools (search_index and vault_map)

**Files:**
- Create: `src/tools/search_index.rs`
- Create: `src/tools/vault_map.rs`
- Modify: `src/tools/mod.rs`

- [ ] **Step 1: Write failing test for search_index**

In `src/tools/search_index.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_indexed_vault(dir: &Path) {
        // Create a chunk file directly
        let chunks_dir = dir.join(".ludolph").join("index").join("chunks").join("notes");
        std::fs::create_dir_all(&chunks_dir).unwrap();
        let chunk_file = serde_json::json!({
            "source": "notes/test.md",
            "source_hash": "abc123",
            "indexed_at": "2026-03-26T00:00:00Z",
            "tier": "standard",
            "frontmatter": {},
            "chunks": [{
                "id": "test-0",
                "heading_path": ["Music Theory", "Chord Progressions"],
                "content": "The II-V-I progression is fundamental to jazz harmony.",
                "char_count": 54,
                "position": 0
            }]
        });
        std::fs::write(chunks_dir.join("test.json"), chunk_file.to_string()).unwrap();
    }

    #[test]
    fn finds_matching_chunks() {
        let tmp = TempDir::new().unwrap();
        create_indexed_vault(tmp.path());

        let input = serde_json::json!({"query": "jazz harmony"});
        let result = execute(&input, tmp.path());
        assert!(result.contains("II-V-I"));
        assert!(result.contains("notes/test.md"));
    }

    #[test]
    fn returns_empty_for_no_matches() {
        let tmp = TempDir::new().unwrap();
        create_indexed_vault(tmp.path());

        let input = serde_json::json!({"query": "quantum physics"});
        let result = execute(&input, tmp.path());
        assert!(result.contains("No matches"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test finds_matching_chunks`
Expected: FAIL

- [ ] **Step 3: Implement search_index tool**

```rust
use crate::index::indexer::{ChunkFile, StoredChunk};
use crate::index::manifest::Manifest;
use serde_json::{json, Value};
use std::path::Path;
use walkdir::WalkDir;

use super::Tool;

pub fn definition() -> Tool {
    Tool {
        name: "search_index".to_string(),
        description: "Search the vault index for matching chunks. Returns relevant sections with \
            source files and heading context. More targeted than full-text search — finds \
            pre-chunked sections of notes with optional AI-generated summaries."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (text or regex pattern)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results to return (default: 10)"
                }
            },
            "required": ["query"]
        }),
    }
}

struct ScoredChunk {
    source: String,
    chunk: StoredChunk,
    score: f64,
}

pub fn execute(input: &Value, vault_path: &Path) -> String {
    let Some(query) = input.get("query").and_then(|v| v.as_str()) else {
        return "Error: 'query' parameter is required".to_string();
    };

    let max_results = input
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    let chunks_dir = Manifest::chunks_dir(vault_path);
    if !chunks_dir.exists() {
        return "Index not found. Run `lu index` to build it. \
            If index is at Quick tier, upgrade with `lu index --tier standard`."
            .to_string();
    }

    let pattern = match regex::RegexBuilder::new(query)
        .case_insensitive(true)
        .build()
    {
        Ok(p) => p,
        Err(_) => {
            let escaped = regex::escape(query);
            match regex::RegexBuilder::new(&escaped)
                .case_insensitive(true)
                .build()
            {
                Ok(p) => p,
                Err(e) => return format!("Error: invalid search pattern: {e}"),
            }
        }
    };

    let mut scored: Vec<ScoredChunk> = Vec::new();

    for entry in WalkDir::new(&chunks_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() || path.extension().is_some_and(|e| e != "json") {
            continue;
        }

        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(chunk_file) = serde_json::from_str::<ChunkFile>(&contents) else {
            continue;
        };

        for chunk in chunk_file.chunks {
            let mut score = 0.0;

            // Check summary first (higher weight)
            if let Some(ref summary) = chunk.summary {
                if pattern.is_match(summary) {
                    score += 2.0;
                }
            }

            // Check content
            if pattern.is_match(&chunk.content) {
                score += 1.0;
            }

            if score > 0.0 {
                // Signal density bonus: shorter chunks with matches are more relevant
                let density = 1.0 / (chunk.char_count as f64 / 100.0).max(1.0);
                score += density * 0.5;

                // Position 0 boost
                if chunk.position == 0 {
                    score += 0.3;
                }

                scored.push(ScoredChunk {
                    source: chunk_file.source.clone(),
                    chunk,
                    score,
                });
            }
        }
    }

    if scored.is_empty() {
        return format!("No matches found for '{query}' in the index.");
    }

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(max_results);

    let mut output = format!("Found {} matches for '{query}':\n\n", scored.len());
    for item in &scored {
        let heading = if item.chunk.heading_path.is_empty() {
            String::new()
        } else {
            format!(" > {}", item.chunk.heading_path.join(" > "))
        };
        output.push_str(&format!("--- {}{} ---\n", item.source, heading));
        if let Some(ref summary) = item.chunk.summary {
            output.push_str(&format!("Summary: {summary}\n"));
        }
        let preview = if item.chunk.content.len() > 300 {
            format!("{}...", &item.chunk.content[..300])
        } else {
            item.chunk.content.clone()
        };
        output.push_str(&format!("{preview}\n\n"));
    }

    output
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test finds_matching_chunks`
Expected: PASS

- [ ] **Step 5: Implement vault_map tool**

In `src/tools/vault_map.rs`:
```rust
use crate::index::manifest::Manifest;
use serde_json::{json, Value};
use std::path::Path;

use super::Tool;

pub fn definition() -> Tool {
    Tool {
        name: "vault_map".to_string(),
        description: "Get a high-level overview of the vault: structure, folder breakdown, \
            index status, and statistics. Use this to understand the vault's layout before \
            diving into specific files."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    }
}

pub fn execute(_input: &Value, vault_path: &Path) -> String {
    let index_dir = Manifest::index_dir(vault_path);

    match Manifest::load(&index_dir) {
        Ok(manifest) => {
            let mut output = format!(
                "Vault: {}\nIndex tier: {}\nFiles: {}\nChunks: {}\nLast indexed: {}\n",
                manifest.vault_path.display(),
                manifest.tier,
                manifest.file_count,
                manifest.chunk_count,
                manifest.last_indexed,
            );

            if !manifest.folders.is_empty() {
                output.push_str("\nFolders:\n");
                let mut folders: Vec<_> = manifest.folders.iter().collect();
                folders.sort_by(|a, b| b.1.file_count.cmp(&a.1.file_count));
                for (folder, stats) in folders {
                    output.push_str(&format!(
                        "  {folder}: {} files, {} chunks\n",
                        stats.file_count, stats.chunk_count
                    ));
                }
            }

            output
        }
        Err(_) => {
            "No vault index found. Run `lu index` to build one.\n\
             Available tiers:\n\
             - quick: file map only (free, seconds)\n\
             - standard: chunked index (free, minutes)\n\
             - deep: chunked + AI summaries (costs API tokens, hours)"
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn returns_manifest_data() {
        let tmp = TempDir::new().unwrap();
        let index_dir = Manifest::index_dir(tmp.path());
        let manifest = Manifest::new(tmp.path(), crate::config::IndexTier::Standard);
        manifest.save(&index_dir).unwrap();

        let input = json!({});
        let result = execute(&input, tmp.path());
        assert!(result.contains("standard"));
        assert!(result.contains("Files: 0"));
    }

    #[test]
    fn returns_guidance_when_no_index() {
        let tmp = TempDir::new().unwrap();
        let input = json!({});
        let result = execute(&input, tmp.path());
        assert!(result.contains("No vault index found"));
        assert!(result.contains("lu index"));
    }
}
```

- [ ] **Step 6: Register both tools in tools/mod.rs**

Add `mod search_index;` and `mod vault_map;` at top.
Add to `get_tool_definitions()`:
```rust
search_index::definition(),
vault_map::definition(),
```
Add to `execute_tool_local()` match:
```rust
"search_index" => search_index::execute(input, vault_path),
"vault_map" => vault_map::execute(input, vault_path),
```

- [ ] **Step 7: Run all tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add src/tools/search_index.rs src/tools/vault_map.rs src/tools/mod.rs
git commit -m "feat: search_index and vault_map Claude tools"
```

---

## Task 7: Setup Integration

**Files:**
- Modify: `src/cli/setup/mod.rs` (or `credentials.rs` — wherever vault path is collected)

- [ ] **Step 1: Add index tier prompt after vault path configuration**

After the vault path is confirmed in setup, add:
```rust
// Count vault files for estimates
let vault_path = std::path::Path::new(&vault_path_str);
let file_count = walkdir::WalkDir::new(vault_path)
    .into_iter()
    .filter_map(Result::ok)
    .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
    .count();

let est_cost = estimate_deep_cost(file_count);
let est_time_standard = if file_count < 1000 { "~30 seconds" } else { "~2 minutes" };
let est_time_deep = if file_count < 1000 { "~30 minutes" } else { "~3 hours" };

println!("\n  Vault found: {} files", file_count);
println!("\n  How should Lu learn your vault?\n");
println!("    1. Quick    — file map only (free, ~5 seconds)");
println!("    2. Standard — chunked index (free, {est_time_standard})");
println!("    3. Deep     — chunked + AI summaries (~${est_cost:.0}, {est_time_deep})");
println!();

let tier_choice = ui::prompt::prompt_with_default("Choose [1/2/3]", "2")?;
let tier = match tier_choice.trim() {
    "1" => IndexTier::Quick,
    "3" => IndexTier::Deep,
    _ => IndexTier::Standard,
};
```

- [ ] **Step 2: Store tier in config and run initial index**

```rust
// Save tier to config
config.index.tier = tier.clone();
config.save()?;

// Run initial index
let spinner = ui::Spinner::new(&format!("Indexing vault ({tier})..."));
let indexer = Indexer::new(vault_path, tier);
match indexer.run(false).await {
    Ok(stats) => {
        spinner.finish();
        println!(
            "  {} files indexed, {} chunks created",
            stats.files_indexed, stats.chunks_created,
        );
    }
    Err(e) => {
        spinner.finish_error();
        tracing::error!("Initial indexing failed: {}", e);
        println!("  Indexing failed: {e}. You can retry with `lu index`.");
    }
}
```

- [ ] **Step 3: Run cargo check**

Run: `cargo check`
Expected: Compiles.

- [ ] **Step 4: Commit**

```bash
git add src/cli/setup/
git commit -m "feat: vault index tier selection in lu setup wizard"
```

---

## Task 8: File Watcher

**Files:**
- Create: `src/index/watcher.rs`
- Modify: `src/bot.rs`

- [ ] **Step 1: Write failing test for watcher exclusion logic**

In `src/index/watcher.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn excludes_hidden_directories() {
        assert!(should_exclude(Path::new(".obsidian/plugins/foo.json")));
        assert!(should_exclude(Path::new(".ludolph/index/chunks/test.json")));
        assert!(should_exclude(Path::new(".trash/old-note.md")));
    }

    #[test]
    fn includes_markdown_files() {
        assert!(!should_exclude(Path::new("notes/todo.md")));
        assert!(!should_exclude(Path::new("daily/2026-03-26.md")));
    }

    #[test]
    fn excludes_non_markdown() {
        assert!(should_exclude(Path::new("images/photo.png")));
        assert!(should_exclude(Path::new("attachments/doc.pdf")));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test excludes_hidden`
Expected: FAIL

- [ ] **Step 3: Implement watcher module**

```rust
use crate::config::IndexTier;
use crate::index::indexer::Indexer;
use crate::index::manifest::Manifest;
use anyhow::Result;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

pub fn should_exclude(path: &Path) -> bool {
    // Check each component for hidden directories
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        if name.starts_with('.') {
            return true;
        }
    }

    // Only include .md files
    if let Some(ext) = path.extension() {
        ext != "md"
    } else {
        true // No extension = exclude
    }
}

pub async fn spawn_watcher(
    vault_path: PathBuf,
    tier: IndexTier,
) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<Vec<PathBuf>>(100);

    let watch_path = vault_path.clone();
    std::thread::spawn(move || {
        let rt_tx = tx.clone();
        let mut debouncer = new_debouncer(
            Duration::from_secs(5),
            move |events: Result<Vec<notify_debouncer_mini::DebouncedEvent>, _>| {
                if let Ok(events) = events {
                    let paths: Vec<PathBuf> = events
                        .into_iter()
                        .filter(|e| e.kind == DebouncedEventKind::Any)
                        .map(|e| e.path)
                        .filter(|p| !should_exclude(p))
                        .collect();

                    if !paths.is_empty() {
                        let _ = rt_tx.blocking_send(paths);
                    }
                }
            },
        )
        .expect("Failed to create file watcher");

        debouncer
            .watcher()
            .watch(&watch_path, notify::RecursiveMode::Recursive)
            .expect("Failed to watch vault directory");

        // Keep the debouncer alive
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    });

    // Process batched changes
    let vault = vault_path.clone();
    tokio::spawn(async move {
        while let Some(changed_paths) = rx.recv().await {
            // Check for lock file — if indexer is running, skip
            let lock_path = Manifest::lock_path(&vault);
            if lock_path.exists() {
                tracing::debug!("Index locked, skipping watcher update for {} files", changed_paths.len());
                continue;
            }

            tracing::debug!("File watcher: {} files changed, re-indexing", changed_paths.len());

            let indexer = Indexer::new(&vault, tier.clone());
            match indexer.run(false).await {
                Ok(stats) => {
                    if stats.files_indexed > 0 || stats.files_removed > 0 {
                        tracing::info!(
                            "Watcher re-indexed: {} files updated, {} removed",
                            stats.files_indexed, stats.files_removed
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Watcher re-index failed: {}", e);
                }
            }
        }
    });

    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test excludes_hidden`
Expected: PASS

- [ ] **Step 5: Integrate watcher into bot.rs**

In `src/bot.rs`, in the `run()` function after config is loaded and before the main bot loop:

```rust
// Spawn file watcher for vault index
if let Some(ref vault_config) = config.vault {
    let vault_path = shellexpand::tilde(&vault_config.path).to_string();
    let vault_path = std::path::PathBuf::from(vault_path);
    if vault_path.exists() {
        let tier = config.index.tier.clone();
        if let Err(e) = crate::index::watcher::spawn_watcher(vault_path, tier).await {
            tracing::warn!("Failed to start vault file watcher: {}", e);
        } else {
            tracing::info!("Vault file watcher started");
        }
    }
}
```

- [ ] **Step 6: Run cargo check and full test suite**

Run: `cargo check && cargo test`
Expected: Compiles, all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/index/watcher.rs src/bot.rs
git commit -m "feat: file watcher for automatic vault re-indexing"
```

---

## Task 9: Deep Tier Enrichment

**Files:**
- Create: `src/index/enricher.rs`
- Modify: `src/index/indexer.rs` (call enricher for Deep tier)

- [ ] **Step 1: Write failing test for enrichment request formatting**

In `src/index/enricher.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_enrichment_prompt() {
        let prompt = build_enrichment_prompt("The II-V-I progression is fundamental to jazz.");
        assert!(prompt.contains("Summarize"));
        assert!(prompt.contains("II-V-I"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test formats_enrichment_prompt`
Expected: FAIL

- [ ] **Step 3: Implement enricher module**

```rust
use crate::index::indexer::StoredChunk;
use anyhow::Result;
use serde::{Deserialize, Serialize};

const ENRICHMENT_PROMPT: &str = "Summarize this chunk in 1-2 sentences. \
    What concept does it capture? What would someone search for to find this?\n\n";

pub fn build_enrichment_prompt(content: &str) -> String {
    format!("{ENRICHMENT_PROMPT}{content}")
}

#[derive(Debug, Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    text: String,
}

pub async fn enrich_batch(
    chunks: &mut [StoredChunk],
    api_key: &str,
) -> Result<usize> {
    let client = reqwest::Client::new();
    let mut enriched = 0;

    for chunk in chunks.iter_mut() {
        let prompt = build_enrichment_prompt(&chunk.content);

        let request = ApiRequest {
            model: "claude-haiku-4-5-20251001".to_string(),
            max_tokens: 100,
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        match client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
        {
            Ok(response) => {
                if let Ok(api_response) = response.json::<ApiResponse>().await {
                    if let Some(block) = api_response.content.first() {
                        chunk.summary = Some(block.text.clone());
                        enriched += 1;
                    }
                } else {
                    tracing::warn!("Enrichment failed for chunk {}: bad response", chunk.id);
                }
            }
            Err(e) => {
                tracing::warn!("Enrichment failed for chunk {}: {}", chunk.id, e);
                // Chunk saved without summary (effectively Standard tier)
            }
        }
    }

    Ok(enriched)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test formats_enrichment_prompt`
Expected: PASS

- [ ] **Step 5: Wire enricher into indexer**

In `src/index/indexer.rs`, after creating the `ChunkFile` and before writing it, add:

```rust
// Deep tier enrichment
if self.tier == IndexTier::Deep {
    let api_key = crate::config::Config::load()
        .ok()
        .and_then(|c| Some(c.claude.api_key.clone()));

    if let Some(key) = api_key {
        let enriched = crate::index::enricher::enrich_batch(
            &mut chunk_file.chunks, &key,
        ).await.unwrap_or(0);

        if enriched < chunk_file.chunks.len() {
            // Some chunks failed enrichment — mark file as standard tier
            chunk_file.tier = "standard".to_string();
        }
    }
}
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All pass (enrichment tests don't hit real API).

- [ ] **Step 7: Commit**

```bash
git add src/index/enricher.rs src/index/indexer.rs
git commit -m "feat: deep tier enrichment via Claude Haiku"
```

---

## Task 10: Quality Gates and Release Verification

**Files:** None new — verification only.

- [ ] **Step 1: Run cargo fmt**

Run: `cargo fmt --check`
Expected: No formatting issues. If there are, run `cargo fmt` to fix.

- [ ] **Step 2: Run cargo clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings. Fix any that appear.

- [ ] **Step 3: Run full test suite**

Run: `cargo test --no-fail-fast`
Expected: All tests pass.

- [ ] **Step 4: Run lu doctor**

Run: `cargo run -- doctor`
Expected: All checks pass.

- [ ] **Step 5: Test lu index manually**

Run against a small test vault or fixture:
```bash
cargo run -- index --tier standard --status
cargo run -- index --tier standard
cargo run -- index --status
```
Expected: Index created, status shows file/chunk counts.

- [ ] **Step 6: Test lu index --tier quick**

```bash
cargo run -- index --tier quick --rebuild
cargo run -- index --status
```
Expected: Only manifest, zero chunks.

- [ ] **Step 7: Verify incremental indexing**

Modify a file in the test vault, re-run `cargo run -- index`. Verify only changed file is re-indexed.

- [ ] **Step 8: Final commit if any fixes were needed**

```bash
git add -A
git commit -m "fix: quality gate fixes for vault index"
```

- [ ] **Step 9: Push branch**

```bash
git push -u origin feature/evaluate-candlekeeps-bookreading-approach
```

- [ ] **Step 10: Send completion notification**

```
mcp__ludolph__lu_send(message="DONE lud-0326a: Vault index implementation complete — lu index command, search_index/vault_map tools, file watcher, setup integration, all tests passing")
```
