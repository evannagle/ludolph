use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::path::Path;

use super::{safe_resolve, Tool};

pub fn definition() -> Tool {
    Tool {
        name: "search".to_string(),
        description: "Search for text in vault files. Returns matching file paths and line numbers."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Text or regex pattern to search for (case-insensitive)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in, relative to vault root. Defaults to entire vault."
                }
            },
            "required": ["query"]
        }),
    }
}

pub fn execute(input: &Value, vault_path: &Path) -> String {
    let query = match input.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return "Error: 'query' parameter is required".to_string(),
    };

    let pattern = match RegexBuilder::new(query).case_insensitive(true).build() {
        Ok(p) => p,
        Err(_) => {
            // Fall back to literal search if invalid regex
            match RegexBuilder::new(&regex::escape(query))
                .case_insensitive(true)
                .build()
            {
                Ok(p) => p,
                Err(e) => return format!("Error building search pattern: {e}"),
            }
        }
    };

    let search_path = match input.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() && p != "." => match safe_resolve(vault_path, p) {
            Some(path) => path,
            None => return format!("Error: Access denied - path '{p}' is outside the vault"),
        },
        _ => vault_path.to_path_buf(),
    };

    let mut results = Vec::new();

    let walker = WalkBuilder::new(&search_path)
        .hidden(true) // Skip hidden files
        .git_ignore(true) // Respect .gitignore
        .build();

    for entry in walker.flatten() {
        let path = entry.path();

        // Only search markdown files
        if path.extension().map_or(false, |e| e == "md") {
            search_file(path, vault_path, &pattern, &mut results);
        }
    }

    if results.is_empty() {
        format!("No matches found for '{query}'")
    } else {
        results.join("\n")
    }
}

fn search_file(file_path: &Path, vault_root: &Path, pattern: &regex::Regex, results: &mut Vec<String>) {
    let contents = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let relative_path = file_path
        .strip_prefix(vault_root)
        .unwrap_or(file_path)
        .to_string_lossy();

    for (line_num, line) in contents.lines().enumerate() {
        if pattern.is_match(line) {
            let preview = if line.len() > 100 {
                format!("{}...", &line[..100])
            } else {
                line.to_string()
            };
            results.push(format!("{}:{}: {}", relative_path, line_num + 1, preview));
        }
    }
}
