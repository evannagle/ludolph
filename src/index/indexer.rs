//! Indexer orchestrator — walks the vault, detects staleness, coordinates chunking,
//! and writes chunk JSON files mirroring the vault folder structure.
//!
//! This module is built incrementally; some items are used in later tasks.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::config::IndexTier;
use crate::index::chunker::{Chunk, chunk_markdown, parse_frontmatter};
use crate::index::manifest::{FolderStats, Manifest};

/// Maximum chunks stored per source file. Files exceeding this emit a warning.
const MAX_CHUNKS_PER_FILE: usize = 200;

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// Serialised representation of all chunks for a single source file.
#[derive(Debug, Serialize, Deserialize)]
pub struct ChunkFile {
    pub source: String,
    pub source_hash: String,
    pub indexed_at: String,
    pub tier: String,
    pub frontmatter: HashMap<String, String>,
    pub chunks: Vec<StoredChunk>,
}

/// A single chunk as it is stored in a chunk JSON file.
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

/// Summary statistics returned after an indexing run.
#[derive(Debug, Default)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub chunks_created: usize,
    pub files_removed: usize,
}

// ---------------------------------------------------------------------------
// Indexer
// ---------------------------------------------------------------------------

/// Orchestrates vault indexing: staleness detection, chunking, and manifest updates.
pub struct Indexer {
    vault_path: PathBuf,
    tier: IndexTier,
}

impl Indexer {
    /// Create a new indexer for the given vault path and index tier.
    #[must_use]
    pub const fn new(vault_path: PathBuf, tier: IndexTier) -> Self {
        Self { vault_path, tier }
    }

    // -----------------------------------------------------------------------
    // Public async API
    // -----------------------------------------------------------------------

    /// Run a full (or rebuild) indexing pass over the vault.
    ///
    /// * `rebuild` — when `true`, the existing chunks directory is deleted before
    ///   indexing, forcing every file to be re-processed.
    pub async fn run(&self, rebuild: bool) -> Result<IndexStats> {
        let _lock = Manifest::acquire_lock(&self.vault_path)?;

        let index_dir = Manifest::index_dir(&self.vault_path);
        let chunks_dir = Manifest::chunks_dir(&self.vault_path);

        // Optionally wipe existing chunks so the run is a clean rebuild.
        if rebuild && chunks_dir.exists() {
            std::fs::remove_dir_all(&chunks_dir)
                .context("Failed to remove existing chunks directory")?;
        }

        // Quick tier: only count files and produce folder stats — no chunk files.
        if self.tier == IndexTier::Quick {
            return self.run_quick(&index_dir);
        }

        // Standard / Deep: walk and chunk.
        self.run_standard_or_deep(&index_dir, &chunks_dir).await
    }

    /// Re-index only the specified paths (for use by the file watcher).
    ///
    /// Deleted files have their chunk files removed; changed/new files are
    /// re-chunked. The manifest is updated after processing.
    pub async fn run_incremental(&self, changed_paths: &[PathBuf]) -> Result<usize> {
        let _lock = Manifest::acquire_lock(&self.vault_path)?;

        let index_dir = Manifest::index_dir(&self.vault_path);
        let chunks_dir = Manifest::chunks_dir(&self.vault_path);
        std::fs::create_dir_all(&chunks_dir)
            .context("Failed to create chunks directory")?;

        let mut chunks_written = 0usize;

        for path in changed_paths {
            let chunk_path = chunk_file_path(&chunks_dir, path);

            if !path.exists() {
                // File was deleted — remove its chunk file.
                if chunk_path.exists() {
                    std::fs::remove_file(&chunk_path)
                        .context("Failed to remove stale chunk file")?;
                }
                continue;
            }

            if !is_markdown(path) {
                continue;
            }

            if self.tier == IndexTier::Deep {
                if let Ok(config) = crate::config::Config::load() {
                    let mut chunk_file = self.build_chunk_file(path)?;
                    let enriched = crate::index::enricher::enrich_batch(
                        &mut chunk_file.chunks,
                        &config.claude.api_key,
                    )
                    .await
                    .unwrap_or(0);

                    if enriched < chunk_file.chunks.len() {
                        chunk_file.tier = "standard".to_string();
                    }

                    chunks_written += chunk_file.chunks.len();
                    write_chunk_file(&chunk_file, &chunks_dir)?;
                    continue;
                }
            }

            let n = self.index_file(path, &chunks_dir)?;
            chunks_written += n;
        }

        // Rebuild manifest totals from the on-disk state.
        self.update_manifest_from_disk(&index_dir, &chunks_dir)?;

        Ok(chunks_written)
    }

    // -----------------------------------------------------------------------
    // Private — tier-specific runners
    // -----------------------------------------------------------------------

    /// Quick tier: manifest only (file count + folder breakdown), no chunk files.
    fn run_quick(&self, index_dir: &Path) -> Result<IndexStats> {
        let file_count = self.count_markdown_files();
        let folders = self.compute_folder_stats_quick();

        let mut manifest = Manifest::new(self.vault_path.clone(), self.tier);
        manifest.file_count = file_count;
        manifest.chunk_count = 0;
        manifest.folders = folders;
        manifest.save(index_dir)?;

        Ok(IndexStats {
            files_indexed: file_count,
            files_skipped: 0,
            chunks_created: 0,
            files_removed: 0,
        })
    }

    /// Standard / Deep tier: incremental walk + chunk files.
    async fn run_standard_or_deep(
        &self,
        index_dir: &Path,
        chunks_dir: &Path,
    ) -> Result<IndexStats> {
        std::fs::create_dir_all(chunks_dir)
            .context("Failed to create chunks directory")?;

        // Load hashes from existing chunk files so we can skip unchanged files.
        let existing_hashes = load_existing_hashes(chunks_dir);

        let mut stats = IndexStats::default();

        // Collect Deep-tier API key once (avoid loading config per file).
        let deep_api_key: Option<String> = if self.tier == IndexTier::Deep {
            crate::config::Config::load()
                .ok()
                .map(|c| c.claude.api_key)
        } else {
            None
        };

        for entry in WalkDir::new(&self.vault_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !should_exclude(e.path()))
        {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();

            if !path.is_file() || !is_markdown(path) {
                continue;
            }

            let relative = path
                .strip_prefix(&self.vault_path)
                .context("Path not under vault")?;
            let source_key = relative.to_string_lossy().to_string();

            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let hash = hash_content(&content);

            // Skip unchanged files.
            if existing_hashes.get(&source_key).is_some_and(|h| h == &hash) {
                stats.files_skipped += 1;
                continue;
            }

            if let Some(api_key) = &deep_api_key {
                let mut chunk_file = self.build_chunk_file(path)?;
                let enriched = crate::index::enricher::enrich_batch(
                    &mut chunk_file.chunks,
                    api_key,
                )
                .await
                .unwrap_or(0);

                if enriched < chunk_file.chunks.len() {
                    chunk_file.tier = "standard".to_string();
                }

                stats.files_indexed += 1;
                stats.chunks_created += chunk_file.chunks.len();
                write_chunk_file(&chunk_file, chunks_dir)?;
            } else {
                let n = self.index_file(path, chunks_dir)?;
                stats.files_indexed += 1;
                stats.chunks_created += n;
            }
        }

        // Remove chunk files for vault files that no longer exist.
        stats.files_removed = cleanup_removed_files(&self.vault_path, chunks_dir)?;

        // Rebuild manifest from disk so totals are always accurate.
        self.update_manifest_from_disk(index_dir, chunks_dir)?;

        Ok(stats)
    }

    // -----------------------------------------------------------------------
    // Private — per-file processing
    // -----------------------------------------------------------------------

    /// Build a `ChunkFile` for a single markdown file (without writing to disk).
    fn build_chunk_file(&self, path: &Path) -> Result<ChunkFile> {
        let relative = path
            .strip_prefix(&self.vault_path)
            .context("Path not under vault")?;

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let hash = hash_content(&content);

        let file_stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let doc = parse_frontmatter(&content);
        let frontmatter = doc.metadata;

        let mut chunks: Vec<StoredChunk> = chunk_markdown(&content, &file_stem)
            .into_iter()
            .map(StoredChunk::from)
            .collect();

        if chunks.len() > MAX_CHUNKS_PER_FILE {
            tracing::warn!(
                path = %path.display(),
                count = chunks.len(),
                max = MAX_CHUNKS_PER_FILE,
                "File exceeds maximum chunk limit; truncating"
            );
            chunks.truncate(MAX_CHUNKS_PER_FILE);
        }

        Ok(ChunkFile {
            source: relative.to_string_lossy().to_string(),
            source_hash: hash,
            indexed_at: Utc::now().to_rfc3339(),
            tier: self.tier.to_string(),
            frontmatter,
            chunks,
        })
    }

    /// Chunk a single markdown file and write its chunk JSON file.
    ///
    /// Returns the number of chunks written.
    fn index_file(&self, path: &Path, chunks_dir: &Path) -> Result<usize> {
        let chunk_file = self.build_chunk_file(path)?;
        let chunk_count = chunk_file.chunks.len();
        write_chunk_file(&chunk_file, chunks_dir)?;
        Ok(chunk_count)
    }

    // -----------------------------------------------------------------------
    // Private — manifest helpers
    // -----------------------------------------------------------------------

    /// Recompute manifest totals from on-disk chunk files and save.
    fn update_manifest_from_disk(&self, index_dir: &Path, chunks_dir: &Path) -> Result<()> {
        let (file_count, chunk_count, folders) = collect_disk_stats(chunks_dir)?;

        let mut manifest = Manifest::new(self.vault_path.clone(), self.tier);
        manifest.file_count = file_count;
        manifest.chunk_count = chunk_count;
        manifest.folders = folders;
        manifest.save(index_dir)?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Private — Quick-tier helpers
    // -----------------------------------------------------------------------

    /// Count the total number of markdown files in the vault.
    fn count_markdown_files(&self) -> usize {
        WalkDir::new(&self.vault_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !should_exclude(e.path()))
            .filter_map(Result::ok)
            .filter(|e| e.path().is_file() && is_markdown(e.path()))
            .count()
    }

    /// Build per-folder file counts (Quick tier — no chunk data available).
    fn compute_folder_stats_quick(&self) -> HashMap<String, FolderStats> {
        let mut folders: HashMap<String, FolderStats> = HashMap::new();

        for entry in WalkDir::new(&self.vault_path)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !should_exclude(e.path()))
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !path.is_file() || !is_markdown(path) {
                continue;
            }
            let Ok(relative) = path.strip_prefix(&self.vault_path) else {
                continue;
            };
            let folder = top_folder(&relative.to_string_lossy());
            let entry = folders.entry(folder).or_insert(FolderStats {
                file_count: 0,
                chunk_count: 0,
            });
            entry.file_count += 1;
        }

        folders
    }
}

// ---------------------------------------------------------------------------
// Free-standing helpers (not `self` methods — pure functions)
// ---------------------------------------------------------------------------

/// Write a `ChunkFile` to disk under `chunks_dir`, creating parent directories as needed.
fn write_chunk_file(chunk_file: &ChunkFile, chunks_dir: &Path) -> Result<()> {
    let relative = Path::new(&chunk_file.source);
    let out_path = chunk_file_path(chunks_dir, relative);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create dir {}", parent.display()))?;
    }

    let json =
        serde_json::to_string_pretty(chunk_file).context("Failed to serialise chunk file")?;
    std::fs::write(&out_path, json)
        .with_context(|| format!("Failed to write chunk file {}", out_path.display()))?;

    Ok(())
}

/// Compute an xxh3 hex hash of `content`.
fn hash_content(content: &str) -> String {
    format!("{:x}", xxhash_rust::xxh3::xxh3_64(content.as_bytes()))
}

/// Returns the first path component of a relative path string (the "top folder").
///
/// If the path has no parent component (i.e. it is a top-level file), returns `"/"`.
fn top_folder(relative: &str) -> String {
    let path = Path::new(relative);
    let component_count = path.components().count();
    let Some(first) = path.components().next() else {
        return "/".to_owned();
    };
    if component_count == 1 {
        "/".to_owned()
    } else {
        first.as_os_str().to_string_lossy().into_owned()
    }
}

/// Returns `true` if `path` has a `.md` extension (case-insensitive).
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Returns `true` if the directory entry should be excluded from indexing.
///
/// Checks only the entry's own name (the last component), so the vault root's
/// absolute path components are not considered. Excludes:
/// - Dotfolders: `.obsidian`, `.ludolph`, `.git`, etc.
/// - `node_modules`
fn should_exclude(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let s = name.to_string_lossy();
    (s.starts_with('.') && s.len() > 1) || s == "node_modules"
}

/// Derive the chunk JSON file path from a relative markdown path.
///
/// Example: `notes/todo.md` → `<chunks_dir>/notes/todo.json`
fn chunk_file_path(chunks_dir: &Path, relative_md: &Path) -> PathBuf {
    chunks_dir.join(relative_md.with_extension("json"))
}

/// Read all existing chunk files and return a map of `source → source_hash`.
fn load_existing_hashes(chunks_dir: &Path) -> HashMap<String, String> {
    let mut hashes = HashMap::new();

    if !chunks_dir.exists() {
        return hashes;
    }

    for entry in WalkDir::new(chunks_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(json) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(cf) = serde_json::from_str::<ChunkFile>(&json) else {
            continue;
        };
        hashes.insert(cf.source, cf.source_hash);
    }

    hashes
}

/// Remove chunk files whose vault source no longer exists.
///
/// Returns the number of chunk files removed.
fn cleanup_removed_files(vault_path: &Path, chunks_dir: &Path) -> Result<usize> {
    if !chunks_dir.exists() {
        return Ok(0);
    }

    let mut removed = 0usize;

    // Collect paths first to avoid mutating during iteration.
    let candidates: Vec<PathBuf> = WalkDir::new(chunks_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.path().is_file()
                && e.path().extension().and_then(|x| x.to_str()) == Some("json")
        })
        .map(walkdir::DirEntry::into_path)
        .collect();

    for chunk_path in candidates {
        // Derive what the vault source path would be.
        let Ok(rel_chunk) = chunk_path.strip_prefix(chunks_dir) else {
            continue;
        };
        // Replace .json extension with .md
        let rel_md = rel_chunk.with_extension("md");
        let vault_file = vault_path.join(&rel_md);

        if !vault_file.exists() {
            std::fs::remove_file(&chunk_path)
                .with_context(|| format!("Failed to remove {}", chunk_path.display()))?;
            removed += 1;
        }
    }

    Ok(removed)
}

/// Walk the chunks directory and aggregate file / chunk counts per folder.
fn collect_disk_stats(
    chunks_dir: &Path,
) -> Result<(usize, usize, HashMap<String, FolderStats>)> {
    let mut file_count = 0usize;
    let mut chunk_count = 0usize;
    let mut folders: HashMap<String, FolderStats> = HashMap::new();

    if !chunks_dir.exists() {
        return Ok((file_count, chunk_count, folders));
    }

    for entry in WalkDir::new(chunks_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let json = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read chunk file {}", path.display()))?;
        let cf: ChunkFile =
            serde_json::from_str(&json)
                .with_context(|| format!("Failed to parse chunk file {}", path.display()))?;

        let n_chunks = cf.chunks.len();
        chunk_count += n_chunks;
        file_count += 1;

        let folder = top_folder(&cf.source);
        let entry = folders.entry(folder).or_insert(FolderStats {
            file_count: 0,
            chunk_count: 0,
        });
        entry.file_count += 1;
        entry.chunk_count += n_chunks;
    }

    Ok((file_count, chunk_count, folders))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_vault() -> TempDir {
        tempfile::tempdir().expect("Failed to create temp dir")
    }

    fn write_file(dir: &Path, relative: &str, content: &str) {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn indexer(vault: &TempDir, tier: IndexTier) -> Indexer {
        Indexer::new(vault.path().to_path_buf(), tier)
    }

    // -----------------------------------------------------------------------
    // indexes_single_file
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn indexes_single_file() {
        let vault = make_vault();
        write_file(
            vault.path(),
            "notes/hello.md",
            "# Hello\n\nThis is my note about something interesting and worth indexing.",
        );

        let stats = indexer(&vault, IndexTier::Standard)
            .run(false)
            .await
            .unwrap();

        let chunks_dir = Manifest::chunks_dir(vault.path());
        let chunk_file = chunks_dir.join("notes/hello.json");

        assert!(
            chunk_file.exists(),
            "Chunk file should exist at {chunk_file:?}"
        );
        assert!(stats.files_indexed >= 1, "At least one file indexed");
        assert!(stats.chunks_created >= 1, "At least one chunk created");

        // Validate the written JSON is valid.
        let json = fs::read_to_string(&chunk_file).unwrap();
        let cf: ChunkFile = serde_json::from_str(&json).unwrap();
        assert_eq!(cf.source, "notes/hello.md");
        assert!(!cf.source_hash.is_empty());
        assert!(!cf.chunks.is_empty());
    }

    // -----------------------------------------------------------------------
    // incremental_skips_unchanged_files
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn incremental_skips_unchanged_files() {
        let vault = make_vault();
        write_file(
            vault.path(),
            "notes/hello.md",
            "# Hello\n\nThis is my note about something interesting and worth indexing.",
        );

        // First run — indexes the file.
        indexer(&vault, IndexTier::Standard)
            .run(false)
            .await
            .unwrap();

        // Second run — file unchanged, should be skipped.
        let stats = indexer(&vault, IndexTier::Standard)
            .run(false)
            .await
            .unwrap();

        assert!(
            stats.files_skipped >= 1,
            "Unchanged file should be skipped on second run"
        );
        assert_eq!(
            stats.files_indexed, 0,
            "No files should be re-indexed when unchanged"
        );
    }

    // -----------------------------------------------------------------------
    // rebuild_clears_existing_index
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn rebuild_clears_existing_index() {
        let vault = make_vault();
        write_file(vault.path(), "notes/hello.md", "# Hello\n\nContent here.");

        // First run — creates chunk.
        indexer(&vault, IndexTier::Standard)
            .run(false)
            .await
            .unwrap();

        let chunks_dir = Manifest::chunks_dir(vault.path());
        assert!(chunks_dir.exists(), "Chunks dir should exist after first run");

        // Add a second file, then index.
        write_file(vault.path(), "notes/other.md", "# Other\n\nOther content.");
        indexer(&vault, IndexTier::Standard)
            .run(false)
            .await
            .unwrap();

        let other_chunk = chunks_dir.join("notes/other.json");
        assert!(other_chunk.exists(), "other.json should exist");

        // Delete the vault file and do a rebuild — rebuild wipes all old chunks first.
        fs::remove_file(vault.path().join("notes/other.md")).unwrap();

        let stats = indexer(&vault, IndexTier::Standard)
            .run(true)
            .await
            .unwrap();

        assert!(
            !other_chunk.exists(),
            "Stale chunk file should be gone after rebuild"
        );
        assert!(stats.files_indexed >= 1, "Remaining file should be indexed");
    }

    // -----------------------------------------------------------------------
    // quick_tier_produces_manifest_only
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn quick_tier_produces_manifest_only() {
        let vault = make_vault();
        write_file(vault.path(), "notes/hello.md", "# Hello\n\nContent.");
        write_file(vault.path(), "notes/world.md", "# World\n\nMore content.");

        let stats = indexer(&vault, IndexTier::Quick)
            .run(false)
            .await
            .unwrap();

        let chunks_dir = Manifest::chunks_dir(vault.path());
        assert!(
            !chunks_dir.exists(),
            "Quick tier must not create a chunks directory"
        );

        // Manifest should record file count.
        let index_dir = Manifest::index_dir(vault.path());
        let manifest = Manifest::load(&index_dir).unwrap();
        assert_eq!(manifest.file_count, 2, "Manifest should count 2 files");
        assert_eq!(manifest.chunk_count, 0, "Quick tier has no chunks");
        assert_eq!(stats.chunks_created, 0);
    }

    // -----------------------------------------------------------------------
    // excludes_hidden_directories
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn excludes_hidden_directories() {
        let vault = make_vault();
        // Regular file.
        write_file(vault.path(), "notes/hello.md", "# Hello\n\nContent.");
        // Files that must be excluded.
        write_file(
            vault.path(),
            ".obsidian/workspace.md",
            "# Obsidian workspace",
        );
        write_file(vault.path(), ".ludolph/cache.md", "# Cache");

        let stats = indexer(&vault, IndexTier::Standard)
            .run(false)
            .await
            .unwrap();

        // Only the non-hidden file should be indexed.
        let chunks_dir = Manifest::chunks_dir(vault.path());
        let obsidian_chunk = chunks_dir.join(".obsidian/workspace.json");
        let ludolph_chunk = chunks_dir.join(".ludolph/cache.json");

        assert!(
            !obsidian_chunk.exists(),
            ".obsidian files must not be indexed"
        );
        assert!(
            !ludolph_chunk.exists(),
            ".ludolph files must not be indexed"
        );
        assert_eq!(
            stats.files_indexed, 1,
            "Only the visible note should be indexed"
        );
    }

    // -----------------------------------------------------------------------
    // handles_empty_vault
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn handles_empty_vault() {
        let vault = make_vault();

        let stats = indexer(&vault, IndexTier::Standard)
            .run(false)
            .await
            .unwrap();

        assert_eq!(stats.files_indexed, 0);
        assert_eq!(stats.chunks_created, 0);
        assert_eq!(stats.files_removed, 0);

        let index_dir = Manifest::index_dir(vault.path());
        let manifest = Manifest::load(&index_dir).unwrap();
        assert_eq!(manifest.file_count, 0);
        assert_eq!(manifest.chunk_count, 0);
    }

    // -----------------------------------------------------------------------
    // Unit: hash_content is deterministic
    // -----------------------------------------------------------------------

    #[test]
    fn hash_content_is_deterministic() {
        let h1 = hash_content("hello world");
        let h2 = hash_content("hello world");
        let h3 = hash_content("different");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
        assert!(!h1.is_empty());
    }

    // -----------------------------------------------------------------------
    // Unit: top_folder
    // -----------------------------------------------------------------------

    #[test]
    fn top_folder_returns_first_component() {
        assert_eq!(top_folder("notes/todo.md"), "notes");
        assert_eq!(top_folder("projects/rust/readme.md"), "projects");
        assert_eq!(top_folder("standalone.md"), "/");
    }

    // -----------------------------------------------------------------------
    // Unit: should_exclude
    // -----------------------------------------------------------------------

    #[test]
    fn should_exclude_dotfolders_and_node_modules() {
        // WalkDir calls filter_entry with the directory entry itself.
        // For `.obsidian/`, the entry path is `vault/.obsidian` — file_name() is `.obsidian`.
        assert!(should_exclude(Path::new(".obsidian")));
        assert!(should_exclude(Path::new("vault/.ludolph")));
        assert!(should_exclude(Path::new("node_modules")));
        assert!(should_exclude(Path::new("vault/node_modules")));
        assert!(!should_exclude(Path::new("notes")));
        assert!(!should_exclude(Path::new("projects/rust")));
        assert!(!should_exclude(Path::new("notes/hello.md")));
    }
}
