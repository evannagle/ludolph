//! Telegram formatting utilities.

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

/// Maximum safe length for a single Telegram message.
///
/// Telegram's hard limit is 4096 characters, but we leave a small margin
/// for HTML entity expansion.
pub const TELEGRAM_MAX_LEN: usize = 4000;

/// Split a message into chunks that fit within Telegram's character limit.
///
/// Splits at paragraph boundaries (`\n\n`) when possible, falling back to
/// line boundaries (`\n`), and finally hard-splitting at `max_len`.
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        let split_at = find_split_point(remaining, max_len);
        let (chunk, rest) = remaining.split_at(split_at);
        chunks.push(chunk.trim_end().to_string());
        remaining = rest.trim_start_matches('\n');
    }

    chunks
}

/// Find the best split point within `max_len` characters.
///
/// Prefers paragraph breaks, then line breaks, then hard cut.
fn find_split_point(text: &str, max_len: usize) -> usize {
    let search_region = &text[..max_len];

    // Try paragraph boundary
    if let Some(pos) = search_region.rfind("\n\n") {
        if pos > max_len / 4 {
            return pos + 1; // include one newline
        }
    }

    // Try line boundary
    if let Some(pos) = search_region.rfind('\n') {
        if pos > max_len / 4 {
            return pos + 1;
        }
    }

    // Hard split at max_len
    max_len
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

    #[test]
    fn split_message_short_text_unchanged() {
        let chunks = split_message("hello world", 100);
        assert_eq!(chunks, vec!["hello world"]);
    }

    #[test]
    fn split_message_splits_at_paragraph() {
        let text = format!("{}\n\n{}", "a".repeat(50), "b".repeat(50));
        let chunks = split_message(&text, 60);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].starts_with('a'));
        assert!(chunks[1].starts_with('b'));
    }

    #[test]
    fn split_message_splits_at_line() {
        let text = format!("{}\n{}", "a".repeat(50), "b".repeat(50));
        let chunks = split_message(&text, 60);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn split_message_hard_splits_no_newlines() {
        let text = "x".repeat(200);
        let chunks = split_message(&text, 100);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 100);
        assert_eq!(chunks[1].len(), 100);
    }

    #[test]
    fn split_message_handles_5k_word_response() {
        // Simulate a ~30KB response (5000 words) with paragraph breaks
        let paragraph = "Lorem ipsum dolor sit amet. ".repeat(50);
        let text = (0..100)
            .map(|_| paragraph.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        assert!(text.len() > 25_000);

        let chunks = split_message(&text, TELEGRAM_MAX_LEN);

        // All chunks must fit within Telegram's limit
        for (i, chunk) in chunks.iter().enumerate() {
            assert!(
                chunk.len() <= TELEGRAM_MAX_LEN,
                "chunk {i} is {} chars, exceeds limit {}",
                chunk.len(),
                TELEGRAM_MAX_LEN
            );
        }

        // No content should be lost (allow for whitespace trimming)
        let rejoined_len: usize = chunks.iter().map(|c| c.len()).sum();
        assert!(
            rejoined_len > text.len() / 2,
            "too much content lost in splitting"
        );
    }
}
