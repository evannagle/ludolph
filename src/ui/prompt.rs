//! Input prompts with π prefix.

use anyhow::Result;
use console::style;
use dialoguer::Input;

/// Prompt for input with a π prefix and help text.
///
/// # Example
/// ```text
/// π Telegram bot token
///   Get one from @BotFather on Telegram
///   : _
/// ```
pub fn prompt_with_help(label: &str, help: &str) -> Result<String> {
    println!();
    println!("{} {}", style("π").bold(), label);
    println!("  {}", style(help).dim());

    let value: String = Input::new().with_prompt("  ").interact_text()?;
    Ok(value)
}

/// Prompt for confirmation (yes/no).
pub fn confirm(message: &str) -> Result<bool> {
    let result = dialoguer::Confirm::new()
        .with_prompt(format!("{} {}", style("π").bold(), message))
        .default(false)
        .interact()?;
    Ok(result)
}
