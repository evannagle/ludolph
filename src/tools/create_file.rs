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
    if let Some(parent) = full_path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("Error creating directories: {e}");
            }
        }
    }

    // Write the file
    match std::fs::write(&full_path, content) {
        Ok(()) => format!("Created {path} ({} bytes)", content.len()),
        Err(e) => format!("Error creating file: {e}"),
    }
}
