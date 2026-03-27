//! Vault index manifest — tracks which files have been indexed and their content hashes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config::IndexTier;

/// Per-folder statistics stored in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderStats {
    pub file_count: usize,
    pub chunk_count: usize,
}

/// Manifest metadata for a vault index.
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

/// Guard that removes the lock file when dropped.
pub struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[allow(dead_code)] // Methods are used in Task 4 (Indexer); suppress until wired up
impl Manifest {
    /// Create a new manifest with zeroed counts and the current timestamp.
    #[must_use]
    pub fn new(vault_path: PathBuf, tier: IndexTier) -> Self {
        Self {
            vault_path,
            tier,
            file_count: 0,
            chunk_count: 0,
            last_indexed: Utc::now().to_rfc3339(),
            version: 1,
            folders: HashMap::new(),
        }
    }

    /// Save the manifest to `index_dir/manifest.json`, creating directories as needed.
    pub fn save(&self, index_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(index_dir)
            .with_context(|| format!("Failed to create index dir {}", index_dir.display()))?;
        let path = index_dir.join("manifest.json");
        let json = serde_json::to_string_pretty(self).context("Failed to serialize manifest")?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write manifest to {}", path.display()))?;
        Ok(())
    }

    /// Load the manifest from `index_dir/manifest.json`.
    pub fn load(index_dir: &Path) -> Result<Self> {
        let path = index_dir.join("manifest.json");
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read manifest from {}", path.display()))?;
        let manifest: Self =
            serde_json::from_str(&json).context("Failed to deserialize manifest")?;
        Ok(manifest)
    }

    /// Returns the index directory path: `vault_path/.ludolph/index`.
    #[must_use]
    pub fn index_dir(vault_path: &Path) -> PathBuf {
        vault_path.join(".ludolph").join("index")
    }

    /// Returns the chunks directory path: `vault_path/.ludolph/index/chunks`.
    #[must_use]
    pub fn chunks_dir(vault_path: &Path) -> PathBuf {
        vault_path.join(".ludolph").join("index").join("chunks")
    }

    /// Returns the lock file path: `vault_path/.ludolph/index/.lock`.
    #[must_use]
    pub fn lock_path(vault_path: &Path) -> PathBuf {
        vault_path.join(".ludolph").join("index").join(".lock")
    }

    /// Acquire an exclusive lock for indexing. Fails if a lock already exists.
    ///
    /// The returned `LockGuard` removes the lock file when dropped.
    pub fn acquire_lock(vault_path: &Path) -> Result<LockGuard> {
        let lock_path = Self::lock_path(vault_path);

        // Ensure the parent directory exists.
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create lock dir {}", parent.display())
            })?;
        }

        if lock_path.exists() {
            bail!(
                "Index is locked — another indexer may be running ({})",
                lock_path.display()
            );
        }

        let pid = std::process::id().to_string();
        std::fs::write(&lock_path, &pid)
            .with_context(|| format!("Failed to write lock file {}", lock_path.display()))?;

        Ok(LockGuard { path: lock_path })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_and_loads_manifest() {
        let dir = tempdir().unwrap();
        let vault_path = dir.path().to_path_buf();
        let index_dir = Manifest::index_dir(&vault_path);

        let manifest = Manifest::new(vault_path.clone(), IndexTier::Standard);
        assert_eq!(manifest.tier, IndexTier::Standard);
        assert_eq!(manifest.file_count, 0);
        assert_eq!(manifest.chunk_count, 0);
        assert_eq!(manifest.version, 1);

        manifest.save(&index_dir).unwrap();

        let loaded = Manifest::load(&index_dir).unwrap();
        assert_eq!(loaded.vault_path, vault_path);
        assert_eq!(loaded.tier, IndexTier::Standard);
        assert_eq!(loaded.file_count, 0);
        assert_eq!(loaded.chunk_count, 0);
        assert_eq!(loaded.version, 1);
        assert!(!loaded.last_indexed.is_empty());
    }

    #[test]
    fn lock_prevents_concurrent_access() {
        let dir = tempdir().unwrap();
        let vault_path = dir.path();

        let _guard = Manifest::acquire_lock(vault_path).unwrap();
        let second = Manifest::acquire_lock(vault_path);

        assert!(second.is_err(), "Second acquire should fail while lock held");
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = tempdir().unwrap();
        let vault_path = dir.path();

        {
            let _guard = Manifest::acquire_lock(vault_path).unwrap();
            // Lock file exists inside this scope.
            assert!(Manifest::lock_path(vault_path).exists());
        }
        // Guard dropped — lock file should be gone.
        assert!(!Manifest::lock_path(vault_path).exists());

        // Should be able to acquire again.
        let second = Manifest::acquire_lock(vault_path);
        assert!(second.is_ok(), "Should acquire lock after previous guard drops");
    }

    #[test]
    fn index_dir_path_is_correct() {
        let vault = PathBuf::from("/home/user/vault");
        let expected = PathBuf::from("/home/user/vault/.ludolph/index");
        assert_eq!(Manifest::index_dir(&vault), expected);
    }

    #[test]
    fn chunks_dir_path_is_correct() {
        let vault = PathBuf::from("/home/user/vault");
        let expected = PathBuf::from("/home/user/vault/.ludolph/index/chunks");
        assert_eq!(Manifest::chunks_dir(&vault), expected);
    }
}
