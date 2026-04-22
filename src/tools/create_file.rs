//! Create a new file in the vault.

use serde_json::{Value, json};
use std::path::Path;

use super::{Tool, safe_resolve};

pub fn definition() -> Tool {
    Tool {
        name: "create_file".to_string(),
        description: "Create a new file in the vault".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path for the new file, relative to vault root"
                },
                "content": {
                    "type": "string",
                    "description": "Initial content for the file"
                }
            },
            "required": ["path", "content"]
        }),
    }
}

pub fn execute(input: &Value, vault_path: &Path) -> String {
    let Some(path) = input.get("path").and_then(|v| v.as_str()) else {
        return "Error: 'path' parameter is required".to_string();
    };

    let Some(content) = input.get("content").and_then(|v| v.as_str()) else {
        return "Error: 'content' parameter is required".to_string();
    };

    let Some(full_path) = safe_resolve(vault_path, path) else {
        return format!("Error: Access denied - path '{path}' is outside the vault");
    };

    // Don't overwrite existing files
    if full_path.exists() {
        return format!("Error: File already exists: {path}");
    }

    // Create parent directories if needed
    if let Some(parent) = full_path.parent()
        && !parent.exists()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return format!("Error creating directories: {e}");
    }

    // Write the file
    match std::fs::write(&full_path, content) {
        Ok(()) => format!("Created {path} ({} bytes)", content.len()),
        Err(e) => format!("Error creating file: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn create_file_handles_5k_word_content() {
        let vault = tempdir().unwrap();
        // Create parent dir so safe_resolve can canonicalize it
        std::fs::create_dir_all(vault.path().join("essays")).unwrap();

        let word = "lorem ";
        let content = word.repeat(5000); // ~30KB
        assert!(content.len() > 25_000);

        let input = json!({
            "path": "essays/long-essay.md",
            "content": content,
        });

        let result = execute(&input, vault.path());
        assert!(
            result.starts_with("Created essays/long-essay.md"),
            "got: {result}"
        );

        // Verify file was written completely
        let written = std::fs::read_to_string(vault.path().join("essays/long-essay.md")).unwrap();
        assert_eq!(written.len(), content.len());
    }

    #[test]
    fn create_file_handles_10k_word_content() {
        let vault = tempdir().unwrap();
        let content = "word ".repeat(10_000); // ~50KB

        let input = json!({
            "path": "big.md",
            "content": content,
        });

        let result = execute(&input, vault.path());
        assert!(result.contains("50000 bytes"), "got: {result}");
    }
}
