//! File watcher — monitors the vault for changes and triggers re-indexing.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};

use crate::config::IndexTier;
use crate::index::indexer::Indexer;
use crate::index::manifest::Manifest;

/// Returns `true` if the path should be excluded from watching.
///
/// Excludes:
/// - Any path whose components contain a segment starting with `.`
/// - Files without a `.md` extension
pub fn should_exclude(path: &Path) -> bool {
    // Exclude any path component that starts with '.'
    for component in path.components() {
        let s = component.as_os_str().to_string_lossy();
        if s.starts_with('.') && s.len() > 1 {
            return true;
        }
    }

    // Only include .md files
    path.extension()
        .and_then(|e| e.to_str())
        .is_none_or(|ext| !ext.eq_ignore_ascii_case("md"))
}

/// Spawn a background file watcher that triggers incremental re-indexing on vault changes.
///
/// Uses a `std::thread` for the notify debouncer (which requires its own OS thread)
/// and a `tokio::task` to receive and process batched change events.
///
/// The function signature is `async` so callers can `.await` it; no top-level await
/// is needed here because all async work is dispatched into a `tokio::spawn` block.
#[allow(clippy::unused_async)]
pub async fn spawn_watcher(vault_path: PathBuf, tier: IndexTier) -> Result<()> {
    let (tx, rx) = mpsc::channel::<Vec<PathBuf>>();

    let watch_path = vault_path.clone();

    // Spawn an OS thread for the notify watcher — notify's debouncer runs a blocking loop.
    std::thread::spawn(move || {
        let tx_clone = tx.clone();

        let mut debouncer = match new_debouncer(
            Duration::from_secs(5),
            move |res: std::result::Result<
                Vec<notify_debouncer_mini::DebouncedEvent>,
                notify_debouncer_mini::notify::Error,
            >| {
                match res {
                    Ok(events) => {
                        let changed: Vec<PathBuf> = events
                            .into_iter()
                            .filter(|e| {
                                e.kind == DebouncedEventKind::Any && !should_exclude(&e.path)
                            })
                            .map(|e| e.path)
                            .collect();

                        if !changed.is_empty() {
                            let _ = tx_clone.send(changed);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("File watcher error: {}", e);
                    }
                }
            },
        ) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("Failed to create file watcher debouncer: {}", e);
                return;
            }
        };

        if let Err(e) = debouncer
            .watcher()
            .watch(&watch_path, RecursiveMode::Recursive)
        {
            tracing::error!("Failed to watch vault path: {}", e);
            return;
        }

        tracing::debug!("File watcher thread started for {}", watch_path.display());

        // Keep the debouncer alive indefinitely.
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    });

    // Spawn a tokio task to receive batched events and trigger incremental indexing.
    // We wrap the std::sync::mpsc::Receiver in a spawn_blocking call each time we poll,
    // since recv() is blocking and must not run on the async runtime.
    tokio::spawn(async move {
        let vault = vault_path.clone();
        // Wrap rx in an Option so we can move it into spawn_blocking once and get it back.
        let mut rx_opt = Some(rx);

        loop {
            let Some(rx_inner) = rx_opt.take() else {
                break;
            };

            let blocking_result = tokio::task::spawn_blocking(move || {
                let result = rx_inner.recv();
                (rx_inner, result)
            })
            .await;

            match blocking_result {
                Ok((rx_returned, Ok(changed_paths))) => {
                    // Put the receiver back for the next iteration.
                    rx_opt = Some(rx_returned);

                    // Skip if an indexing run is already in progress.
                    if Manifest::lock_path(&vault).exists() {
                        tracing::debug!(
                            "Skipping watcher batch — index lock is held ({} paths)",
                            changed_paths.len()
                        );
                        continue;
                    }

                    match Indexer::new(vault.clone(), tier)
                        .run_incremental(&changed_paths)
                        .await
                    {
                        Ok(n) => {
                            tracing::info!(
                                "Incremental index: {} chunk(s) written for {} changed file(s)",
                                n,
                                changed_paths.len()
                            );
                        }
                        Err(e) => {
                            tracing::error!("Incremental index failed: {}", e);
                        }
                    }
                }
                Ok((_, Err(_))) => {
                    // Channel closed — watcher thread exited.
                    tracing::debug!("File watcher channel closed");
                    break;
                }
                Err(e) => {
                    tracing::error!("File watcher receiver task panicked: {}", e);
                    break;
                }
            }
        }
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_hidden_directories() {
        // Files under hidden directories must be excluded.
        assert!(should_exclude(Path::new(".obsidian/workspace.md")));
        assert!(should_exclude(Path::new(".ludolph/cache.md")));
        assert!(should_exclude(Path::new(".trash/note.md")));
        // A path component like vault/.obsidian/note.md should also be excluded.
        assert!(should_exclude(Path::new("vault/.obsidian/note.md")));
    }

    #[test]
    fn includes_markdown_files() {
        // A plain markdown file in a visible directory should not be excluded.
        assert!(!should_exclude(Path::new("notes/todo.md")));
        assert!(!should_exclude(Path::new("projects/rust/readme.md")));
        assert!(!should_exclude(Path::new("standalone.md")));
    }

    #[test]
    fn excludes_non_markdown() {
        // Non-.md files must be excluded regardless of directory.
        assert!(should_exclude(Path::new("images/photo.png")));
        assert!(should_exclude(Path::new("assets/style.css")));
        assert!(should_exclude(Path::new("notes/data.json")));
        // Files with no extension are also excluded.
        assert!(should_exclude(Path::new("notes/noextension")));
        assert!(should_exclude(Path::new("README")));
    }
}
