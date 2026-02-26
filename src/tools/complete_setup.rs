//! Tool for signaling setup completion.
//!
//! This tool is called by the setup wizard after creating Lu.md
//! to signal that setup mode should exit.

use serde_json::{Value, json};

use super::Tool;

/// Return the tool definition for `complete_setup`.
pub fn definition() -> Tool {
    Tool {
        name: "complete_setup".to_string(),
        description:
            "Signal that setup is complete. Call this after writing Lu.md to exit setup mode."
                .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {},
        }),
    }
}

/// Execute the `complete_setup` tool.
///
/// Returns a marker that the bot uses to detect setup completion.
pub fn execute(_input: &Value, _vault_path: &std::path::Path) -> String {
    // Return the marker that bot.rs will detect
    format!(
        "{} Setup wizard completed successfully.",
        crate::setup::SETUP_COMPLETE_MARKER
    )
}
