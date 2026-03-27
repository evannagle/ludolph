use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::Path;

use super::Tool;
use crate::index::manifest::Manifest;

pub fn definition() -> Tool {
    Tool {
        name: "vault_map".to_string(),
        description:
            "Get a high-level overview of the vault: structure, folder breakdown, index status."
                .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
    }
}

pub fn execute(_input: &Value, vault_path: &Path) -> String {
    let index_dir = Manifest::index_dir(vault_path);

    let Ok(manifest) = Manifest::load(&index_dir) else {
        return "No index found. Run `lu index` to build one.\n\n\
                Available tiers:\n\
                - quick   — file count and folder breakdown only\n\
                - standard — full chunking (recommended)\n\
                - deep    — chunking + AI-generated summaries\n\n\
                Example: lu index --tier standard"
            .to_string();
    };

    let mut output = String::new();

    let _ = writeln!(output, "Vault: {}", manifest.vault_path.display());
    let _ = writeln!(output, "Tier: {}", manifest.tier);
    let _ = writeln!(output, "Files indexed: {}", manifest.file_count);
    let _ = writeln!(output, "Chunks: {}", manifest.chunk_count);
    let _ = writeln!(output, "Last indexed: {}", manifest.last_indexed);

    if !manifest.folders.is_empty() {
        output.push_str("\nFolder breakdown:\n");

        let mut folders: Vec<(&String, &crate::index::manifest::FolderStats)> =
            manifest.folders.iter().collect();
        folders.sort_by(|a, b| b.1.file_count.cmp(&a.1.file_count));

        for (folder, stats) in folders {
            let _ = writeln!(
                output,
                "  {folder}: {} files, {} chunks",
                stats.file_count, stats.chunk_count
            );
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::IndexTier;
    use crate::index::manifest::{FolderStats, Manifest};
    use serde_json::json;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_vault() -> TempDir {
        tempfile::tempdir().expect("Failed to create temp dir")
    }

    #[test]
    fn returns_manifest_data() {
        let vault = make_vault();
        let index_dir = Manifest::index_dir(vault.path());

        let mut manifest = Manifest::new(vault.path().to_path_buf(), IndexTier::Standard);
        manifest.file_count = 42;
        manifest.chunk_count = 137;
        manifest.folders = HashMap::from([
            (
                "notes".to_string(),
                FolderStats {
                    file_count: 30,
                    chunk_count: 100,
                },
            ),
            (
                "projects".to_string(),
                FolderStats {
                    file_count: 12,
                    chunk_count: 37,
                },
            ),
        ]);
        manifest.save(&index_dir).unwrap();

        let input = json!({});
        let result = execute(&input, vault.path());

        assert!(result.contains("42"), "Should show file count");
        assert!(result.contains("137"), "Should show chunk count");
        assert!(result.contains("standard"), "Should show tier");
        assert!(result.contains("notes"), "Should show folder names");
        assert!(result.contains("projects"), "Should show folder names");
        // notes should come before projects (higher file count)
        let pos_notes = result.find("notes").unwrap_or(usize::MAX);
        let pos_projects = result.find("projects").unwrap_or(usize::MAX);
        assert!(
            pos_notes < pos_projects,
            "Folders should be sorted by file count descending"
        );
    }

    #[test]
    fn returns_guidance_when_no_index() {
        let vault = make_vault();
        // No manifest saved

        let input = json!({});
        let result = execute(&input, vault.path());

        assert!(
            result.contains("lu index"),
            "Should guide user to run `lu index`"
        );
        assert!(
            result.contains("standard"),
            "Should mention available tiers"
        );
    }
}
