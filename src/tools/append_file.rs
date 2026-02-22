//! Append content to an existing file in the vault.

use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use super::{Tool, safe_resolve};

pub fn definition() -> Tool {
    Tool {
        name: "append_file".to_string(),
        description: "Append text to an existing file in the vault".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file, relative to vault root"
                },
                "content": {
                    "type": "string",
                    "description": "Text to append to the file"
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

    // File must exist
    if !full_path.exists() {
        return format!("Error: File does not exist: {path}");
    }

    if !full_path.is_file() {
        return format!("Error: Path is not a file: {path}");
    }

    // Read existing content to check if we need a newline
    let existing = match std::fs::read_to_string(&full_path) {
        Ok(c) => c,
        Err(e) => return format!("Error reading file: {e}"),
    };

    let needs_newline = !existing.is_empty() && !existing.ends_with('\n');

    // Append content
    let mut file = match OpenOptions::new().append(true).open(&full_path) {
        Ok(f) => f,
        Err(e) => return format!("Error opening file for append: {e}"),
    };

    let to_write = if needs_newline {
        format!("\n{content}")
    } else {
        content.to_string()
    };

    match file.write_all(to_write.as_bytes()) {
        Ok(()) => format!("Appended {} bytes to {path}", to_write.len()),
        Err(e) => format!("Error writing to file: {e}"),
    }
}
