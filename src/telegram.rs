//! Telegram formatting utilities.

use rand::prelude::IndexedRandom;

/// Convert markdown-style formatting to Telegram HTML.
///
/// Telegram supports a subset of HTML:
/// - `<b>bold</b>`
/// - `<i>italic</i>`
/// - `<code>monospace</code>`
/// - `<pre>code block</pre>`
/// - `<a href="...">link</a>`
pub fn to_telegram_html(text: &str) -> String {
    let mut result = String::with_capacity(text.len());

    // First, escape HTML special characters that aren't part of our formatting
    let text = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");

    let mut chars = text.chars().peekable();
    let mut in_code_block = false;
    let mut in_inline_code = false;

    while let Some(c) = chars.next() {
        match c {
            // Code blocks: ```...```
            '`' if chars.peek() == Some(&'`') => {
                chars.next(); // consume second `
                if chars.peek() == Some(&'`') {
                    chars.next(); // consume third `
                    // Skip optional language identifier
                    while chars.peek().is_some_and(|&c| c != '\n' && c != '`') {
                        chars.next();
                    }
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    if in_code_block {
                        result.push_str("</pre>");
                        in_code_block = false;
                    } else {
                        result.push_str("<pre>");
                        in_code_block = true;
                    }
                } else {
                    // Just two backticks, treat as text
                    result.push_str("``");
                }
            }
            // Inline code: `...`
            '`' if !in_code_block => {
                if in_inline_code {
                    result.push_str("</code>");
                    in_inline_code = false;
                } else {
                    result.push_str("<code>");
                    in_inline_code = true;
                }
            }
            // Bold: **...** or __...__
            '*' if !in_code_block && chars.peek() == Some(&'*') => {
                chars.next();
                result.push_str("<b>");
                // Find closing **
                let mut content = String::new();
                while let Some(c) = chars.next() {
                    if c == '*' && chars.peek() == Some(&'*') {
                        chars.next();
                        break;
                    }
                    content.push(c);
                }
                result.push_str(&content);
                result.push_str("</b>");
            }
            // Italic: *...* or _..._
            '*' if !in_code_block => {
                result.push_str("<i>");
                let mut content = String::new();
                for c in chars.by_ref() {
                    if c == '*' {
                        break;
                    }
                    content.push(c);
                }
                result.push_str(&content);
                result.push_str("</i>");
            }
            // List items: convert "- " at start of line to bullet
            '-' if !in_code_block => {
                if chars.peek() == Some(&' ') {
                    // Check if this is at start of line (result ends with newline or is empty)
                    if result.is_empty() || result.ends_with('\n') {
                        result.push('•');
                    } else {
                        result.push('-');
                    }
                } else {
                    result.push('-');
                }
            }
            _ => result.push(c),
        }
    }

    // Close any unclosed tags
    if in_code_block {
        result.push_str("</pre>");
    }
    if in_inline_code {
        result.push_str("</code>");
    }

    result
}

/// Get a random "thinking" message.
pub fn thinking_message() -> &'static str {
    const MESSAGES: &[&str] = &[
        "Let me check the vault...",
        "Looking through your notes...",
        "Searching the vault...",
        "Let me find that...",
        "Checking your notes...",
        "One moment...",
    ];

    MESSAGES.choose(&mut rand::rng()).unwrap_or(&MESSAGES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bold() {
        assert_eq!(to_telegram_html("**bold**"), "<b>bold</b>");
    }

    #[test]
    fn test_italic() {
        assert_eq!(to_telegram_html("*italic*"), "<i>italic</i>");
    }

    #[test]
    fn test_inline_code() {
        assert_eq!(to_telegram_html("`code`"), "<code>code</code>");
    }

    #[test]
    fn test_list_items() {
        assert_eq!(to_telegram_html("- item"), "• item");
        assert_eq!(to_telegram_html("text\n- item"), "text\n• item");
    }

    #[test]
    fn test_escapes_html() {
        assert_eq!(to_telegram_html("<script>"), "&lt;script&gt;");
    }
}
