use serde_json::{json, Value};
use std::path::Path;
use super::{Tool, safe_resolve};

pub fn definition() -> Tool {
    Tool {
        name: "read_file".to_string(),
        description: "Read the contents of a file from the vault".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file, relative to vault root"
                }
            },
            "required": ["path"]
        }),
    }
}

pub fn execute(input: &Value, vault_path: &Path) -> String {
    let path = match input.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' parameter is required".to_string(),
    };

    let full_path = match safe_resolve(vault_path, path) {
        Some(p) => p,
        None => return format!("Error: Access denied - path '{}' is outside the vault", path),
    };

    match std::fs::read_to_string(&full_path) {
        Ok(contents) => contents,
        Err(e) => format!("Error reading file: {}", e),
    }
}
