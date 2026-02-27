mod append_file;
mod complete_setup;
mod create_file;
mod list_dir;
mod read_file;
mod search;

use serde_json::Value;
use std::fmt::Write as _;

pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Get tool definitions for local execution.
pub fn get_tool_definitions() -> Vec<Tool> {
    vec![
        read_file::definition(),
        list_dir::definition(),
        search::definition(),
        append_file::definition(),
        create_file::definition(),
        complete_setup::definition(),
    ]
}

/// Execute a tool locally (for Mac or standalone Pi with local vault).
pub async fn execute_tool_local(name: &str, input: &Value, vault_path: &std::path::Path) -> String {
    let result = match name {
        "read_file" => read_file::execute(input, vault_path),
        "list_dir" => list_dir::execute(input, vault_path),
        "search" => search::execute(input, vault_path),
        "append_file" => append_file::execute(input, vault_path),
        "create_file" => create_file::execute(input, vault_path),
        "complete_setup" => complete_setup::execute(input, vault_path),
        _ => format!("Unknown tool: {name}"),
    };

    // Format errors with helpful suggestions
    if result.starts_with("Error") {
        format_tool_error(name, &result, input)
    } else {
        result
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

/// Format tool errors with helpful suggestions.
#[allow(clippy::too_many_lines)]
fn format_tool_error(tool_name: &str, error_msg: &str, input: &Value) -> String {
    // Extract the core error message
    let error_text = error_msg.strip_prefix("Error: ").unwrap_or(error_msg);

    // Build the main error message
    let mut formatted = format!("Unable to execute {tool_name}.\n\nError: {error_text}\n\nTry:\n");

    match tool_name {
        "read_file" => {
            if error_text.contains("not found") || error_text.contains("does not exist") {
                formatted.push_str("• Check that the file path is correct\n");
                formatted.push_str("• Use list_dir to see available files\n");
                formatted.push_str("• Ensure the path is relative to vault root\n");
            } else if error_text.contains("Access denied")
                || error_text.contains("outside the vault")
            {
                formatted.push_str("• Path must be within the vault\n");
                formatted.push_str("• Avoid using '..' in paths\n");
                formatted.push_str("• Use relative paths from vault root\n");
            } else if error_text.contains("Permission denied") {
                formatted.push_str("• Check file permissions\n");
                formatted.push_str("• Ensure the file is readable\n");
            } else {
                formatted.push_str("• Verify the file exists and is readable\n");
                formatted.push_str("• Use list_dir to browse available files\n");
            }
        }
        "list_dir" => {
            if error_text.contains("not found") || error_text.contains("does not exist") {
                formatted.push_str("• Check that the directory path is correct\n");
                formatted.push_str("• Use '.' or '' to list vault root\n");
                formatted.push_str("• Try listing the parent directory first\n");
            } else if error_text.contains("Access denied")
                || error_text.contains("outside the vault")
            {
                formatted.push_str("• Path must be within the vault\n");
                formatted.push_str("• Avoid using '..' in paths\n");
                formatted.push_str("• Use relative paths from vault root\n");
            } else if error_text.contains("not a directory") {
                formatted.push_str("• This path is a file, not a directory\n");
                formatted.push_str("• Use read_file to read file contents\n");
            } else {
                formatted.push_str("• Verify the directory exists\n");
                formatted.push_str("• Use '.' to list vault root\n");
            }
        }
        "search" => {
            if error_text.contains("Access denied") || error_text.contains("outside the vault") {
                formatted.push_str("• Search path must be within the vault\n");
                formatted.push_str("• Omit path parameter to search entire vault\n");
            } else if error_text.contains("query") {
                formatted.push_str("• The 'query' parameter is required\n");
                formatted.push_str("• Provide text or regex pattern to search for\n");
            } else {
                formatted.push_str("• Try a different search query\n");
                formatted.push_str("• Omit path parameter to search entire vault\n");
            }
        }
        "append_file" => {
            if error_text.contains("does not exist") {
                formatted.push_str("• Use create_file to create a new file\n");
                formatted.push_str("• Use list_dir to see existing files\n");
                if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                    let _ = writeln!(formatted, "• Create '{path}' first with create_file");
                }
            } else if error_text.contains("not a file") {
                formatted.push_str("• Path points to a directory, not a file\n");
                formatted.push_str("• Append only works with files\n");
            } else if error_text.contains("Access denied")
                || error_text.contains("outside the vault")
            {
                formatted.push_str("• Path must be within the vault\n");
                formatted.push_str("• Avoid using '..' in paths\n");
            } else if error_text.contains("Permission denied") {
                formatted.push_str("• Check file permissions\n");
                formatted.push_str("• Ensure the file is writable\n");
            } else if error_text.contains("parameter") {
                formatted.push_str("• Both 'path' and 'content' are required\n");
            } else {
                formatted.push_str("• Verify the file exists and is writable\n");
                formatted.push_str("• Use read_file to check current contents\n");
            }
        }
        "create_file" => {
            if error_text.contains("already exists") {
                formatted.push_str("• Use append_file to add to existing file\n");
                formatted.push_str("• Choose a different file name\n");
                if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
                    let _ = writeln!(formatted, "• '{path}' already exists");
                }
            } else if error_text.contains("Access denied")
                || error_text.contains("outside the vault")
            {
                formatted.push_str("• Path must be within the vault\n");
                formatted.push_str("• Avoid using '..' in paths\n");
            } else if error_text.contains("creating directories") {
                formatted.push_str("• Parent directory cannot be created\n");
                formatted.push_str("• Check parent path permissions\n");
            } else if error_text.contains("parameter") {
                formatted.push_str("• Both 'path' and 'content' are required\n");
            } else {
                formatted.push_str("• Verify the parent directory exists\n");
                formatted.push_str("• Check write permissions\n");
            }
        }
        "complete_setup" => {
            formatted.push_str("• Verify the setup file is readable\n");
            formatted.push_str("• Check file format and syntax\n");
        }
        _ => {
            formatted.push_str("• Check the tool parameters\n");
            formatted.push_str("• Verify paths are relative to vault root\n");
        }
    }

    formatted
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn error_formatter_adds_suggestions_for_read_file() {
        let temp_dir = std::env::temp_dir().join("ludolph_test_read_file");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let input = json!({"path": "nonexistent.md"});
        let result = execute_tool_local("read_file", &input, &temp_dir).await;

        assert!(result.contains("Unable to execute read_file"));
        assert!(result.contains("Try:"));
        assert!(result.contains("list_dir"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn error_formatter_adds_suggestions_for_list_dir() {
        let temp_dir = std::env::temp_dir().join("ludolph_test_list_dir");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let input = json!({"path": "nonexistent"});
        let result = execute_tool_local("list_dir", &input, &temp_dir).await;

        assert!(result.contains("Unable to execute list_dir"));
        assert!(result.contains("Try:"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn error_formatter_adds_suggestions_for_append_file() {
        let temp_dir = std::env::temp_dir().join("ludolph_test_append");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let input = json!({"path": "missing.md", "content": "test"});
        let result = execute_tool_local("append_file", &input, &temp_dir).await;

        assert!(result.contains("Unable to execute append_file"));
        assert!(result.contains("Try:"));
        assert!(result.contains("create_file"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn error_formatter_adds_suggestions_for_create_file() {
        let temp_dir = std::env::temp_dir().join("ludolph_test_create");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a file that already exists
        let file_path = temp_dir.join("existing.md");
        std::fs::write(&file_path, "content").unwrap();

        let input = json!({"path": "existing.md", "content": "new"});
        let result = execute_tool_local("create_file", &input, &temp_dir).await;

        assert!(result.contains("Unable to execute create_file"));
        assert!(result.contains("Try:"));
        assert!(result.contains("append_file"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[tokio::test]
    async fn successful_operations_return_original_output() {
        let temp_dir = std::env::temp_dir().join("ludolph_test_success");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a file successfully
        let input = json!({"path": "test.md", "content": "test content"});
        let result = execute_tool_local("create_file", &input, &temp_dir).await;

        assert!(!result.contains("Unable to execute"));
        assert!(result.contains("Created test.md"));

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
