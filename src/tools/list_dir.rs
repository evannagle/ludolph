use serde_json::{json, Value};
use std::path::Path;
use super::{Tool, safe_resolve};

pub fn definition() -> Tool {
    Tool {
        name: "list_dir".to_string(),
        description: "List files and directories in a vault path".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory, relative to vault root. Use '.' or '' for vault root."
                }
            },
            "required": ["path"]
        }),
    }
}

pub fn execute(input: &Value, vault_path: &Path) -> String {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let dir_path = if path.is_empty() || path == "." {
        vault_path.to_path_buf()
    } else {
        match safe_resolve(vault_path, path) {
            Some(p) => p,
            None => return format!("Error: Access denied - path '{}' is outside the vault", path),
        }
    };

    match std::fs::read_dir(&dir_path) {
        Ok(entries) => {
            let mut items: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let file_type = e.file_type().ok();
                    if file_type.map(|ft| ft.is_dir()).unwrap_or(false) {
                        format!("{}/", name)
                    } else {
                        name
                    }
                })
                .collect();

            items.sort();
            items.join("\n")
        }
        Err(e) => format!("Error listing directory: {}", e),
    }
}
