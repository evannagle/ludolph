mod append_file;
mod create_file;
mod list_dir;
mod read_file;
mod search;

use serde_json::Value;

pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub fn get_tool_definitions() -> Vec<Tool> {
    vec![
        read_file::definition(),
        list_dir::definition(),
        search::definition(),
        append_file::definition(),
        create_file::definition(),
    ]
}

pub async fn execute_tool(name: &str, input: &Value, vault_path: &std::path::Path) -> String {
    match name {
        "read_file" => read_file::execute(input, vault_path),
        "list_dir" => list_dir::execute(input, vault_path),
        "search" => search::execute(input, vault_path),
        "append_file" => append_file::execute(input, vault_path),
        "create_file" => create_file::execute(input, vault_path),
        _ => format!("Unknown tool: {name}"),
    }
}

/// Resolve a path safely within the vault, preventing directory traversal
pub fn safe_resolve(
    vault_path: &std::path::Path,
    relative_path: &str,
) -> Option<std::path::PathBuf> {
    // Reject paths with ..
    if relative_path.contains("..") {
        return None;
    }

    // Clean the path and join with vault
    let clean_path = relative_path.trim_start_matches('/');
    let full_path = vault_path.join(clean_path);

    // Canonicalize and verify it's still within vault
    if let Ok(canonical) = full_path.canonicalize()
        && let Ok(vault_canonical) = vault_path.canonicalize()
        && canonical.starts_with(&vault_canonical)
    {
        return Some(canonical);
    }

    // For non-existent files, verify the parent is within vault
    if let Some(parent) = full_path.parent()
        && let Ok(parent_canonical) = parent.canonicalize()
        && let Ok(vault_canonical) = vault_path.canonicalize()
        && parent_canonical.starts_with(&vault_canonical)
    {
        return Some(full_path);
    }

    None
}
