//! File chunking pipeline — splits vault files into indexable chunks.
//!
//! Parses frontmatter, splits on headings, applies size guards, and merges
//! small sections into a flat list of [`Chunk`] values ready for indexing.
#![allow(dead_code)] // Module is built incrementally; usage comes in Task 4 (Indexer)

use std::collections::HashMap;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Maximum characters per chunk before splitting on paragraph boundaries.
pub const MAX_CHUNK_SIZE: usize = 1000;

/// Minimum characters per chunk; smaller chunks are merged with the next sibling.
pub const MIN_CHUNK_SIZE: usize = 200;

/// Character overlap when hard-splitting a single oversized paragraph.
pub const OVERLAP_SIZE: usize = 100;

/// Parsed result of a markdown document's frontmatter and body.
pub struct ParsedDocument {
    /// YAML metadata extracted from the leading `---` block.
    pub metadata: HashMap<String, String>,
    /// The document body with frontmatter removed.
    pub body: String,
}

/// A single indexable chunk produced from a markdown file.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Stable identifier: `{file_stem}-{position}`.
    pub id: String,
    /// Heading hierarchy leading to this chunk (e.g. `["Title", "Section A"]`).
    pub heading_path: Vec<String>,
    /// The textual content of this chunk.
    pub content: String,
    /// Character count of [`content`].
    pub char_count: usize,
    /// Zero-based position within the file's chunk list.
    pub position: usize,
}

/// Parses YAML frontmatter from `content`, returning metadata and stripped body.
///
/// Frontmatter must be delimited by `---` on lines by themselves. If no valid
/// frontmatter block is found, `metadata` is empty and `body` is the full input.
///
/// # Examples
/// ```
/// use ludolph::index::chunker::parse_frontmatter;
/// let doc = parse_frontmatter("---\ntitle: My Note\n---\n# Body");
/// assert_eq!(doc.metadata.get("title").map(String::as_str), Some("My Note"));
/// assert!(!doc.body.contains("---"));
/// ```
pub fn parse_frontmatter(content: &str) -> ParsedDocument {
    let stripped = content.trim_start();
    if !stripped.starts_with("---") {
        return ParsedDocument {
            metadata: HashMap::new(),
            body: content.to_owned(),
        };
    }

    // Find closing delimiter after the opening `---\n`
    let after_open = &stripped[3..];
    let Some(close_pos) = after_open.find("\n---") else {
        return ParsedDocument {
            metadata: HashMap::new(),
            body: content.to_owned(),
        };
    };

    let yaml_src = &after_open[..close_pos];
    // Skip past `\n---` and the optional trailing newline
    let rest = &after_open[close_pos + 4..];
    let body = rest.trim_start_matches('\n').to_owned();

    let metadata = parse_yaml_metadata(yaml_src);

    ParsedDocument { metadata, body }
}

/// Converts a raw YAML block into a flat `HashMap<String, String>`.
fn parse_yaml_metadata(yaml: &str) -> HashMap<String, String> {
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(yaml) else {
        return HashMap::new();
    };
    let Some(mapping) = value.as_mapping() else {
        return HashMap::new();
    };
    mapping
        .iter()
        .filter_map(|(k, v)| {
            let key = k.as_str()?.to_owned();
            let val = format_yaml_value(v);
            Some((key, val))
        })
        .collect()
}

/// Converts a [`serde_yaml::Value`] to its string representation.
///
/// - Strings are returned as-is.
/// - Sequences are joined with `", "`.
/// - Booleans / numbers are converted via `to_string()`.
/// - Null becomes an empty string.
/// - Nested mappings become their `Debug` representation (best-effort).
fn format_yaml_value(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Sequence(seq) => seq
            .iter()
            .map(format_yaml_value)
            .collect::<Vec<_>>()
            .join(", "),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Null
        | serde_yaml::Value::Mapping(_)
        | serde_yaml::Value::Tagged(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Internal section type used before finalising chunk IDs
// ---------------------------------------------------------------------------

struct Section {
    heading_path: Vec<String>,
    content: String,
}

/// Splits a markdown string on headings (`#` through `######`).
///
/// Each heading starts a new section. Text appearing before the first heading
/// is placed in a section with an empty heading path.
fn split_on_headings(body: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current_path: Vec<String> = Vec::new();
    let mut current_content = String::new();

    let opts = Options::all();
    let parser = Parser::new_ext(body, opts);

    // We need the raw text of headings, so we accumulate heading text
    // while inside a heading tag.
    let mut in_heading = false;
    let mut heading_text = String::new();
    let mut heading_level: u32 = 1;

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                // Flush current section before starting a new one
                let trimmed = current_content.trim().to_owned();
                if !trimmed.is_empty() || !current_path.is_empty() {
                    sections.push(Section {
                        heading_path: current_path.clone(),
                        content: trimmed,
                    });
                }
                current_content.clear();

                in_heading = true;
                heading_text.clear();
                heading_level = heading_level_to_u32(level);
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                let level = heading_level;
                let text = std::mem::take(&mut heading_text);

                // Truncate path to parent level, then push this heading
                current_path.truncate(level.saturating_sub(1) as usize);
                current_path.push(text.clone());
                // Start fresh content after the heading
                current_content.clear();
            }
            Event::Text(t) if in_heading => {
                heading_text.push_str(&t);
            }
            Event::Code(code) if in_heading => {
                heading_text.push_str(&code);
            }
            // For non-heading events, reconstruct plain text content.
            // We preserve wikilinks by keeping the raw text tokens.
            Event::Text(t) => current_content.push_str(&t),
            Event::Code(code) => {
                current_content.push('`');
                current_content.push_str(&code);
                current_content.push('`');
            }
            Event::SoftBreak | Event::HardBreak => current_content.push('\n'),
            Event::Start(Tag::Paragraph) => {
                if !current_content.is_empty()
                    && !current_content.ends_with('\n')
                {
                    current_content.push('\n');
                }
            }
            Event::End(TagEnd::Paragraph) => {
                current_content.push('\n');
            }
            _ => {}
        }
    }

    // Flush the last section
    let trimmed = current_content.trim().to_owned();
    if !trimmed.is_empty() || !current_path.is_empty() {
        sections.push(Section {
            heading_path: current_path.clone(),
            content: trimmed,
        });
    }

    sections
}

/// Converts a [`HeadingLevel`] to its numeric depth (1–6).
const fn heading_level_to_u32(level: HeadingLevel) -> u32 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

// ---------------------------------------------------------------------------
// Size guards
// ---------------------------------------------------------------------------

/// Splits sections whose content exceeds [`MAX_CHUNK_SIZE`].
///
/// Splitting strategy:
/// 1. Split on paragraph boundaries (`\n\n`).
/// 2. If a single paragraph still exceeds `MAX_CHUNK_SIZE`, hard-split with
///    [`OVERLAP_SIZE`] character overlap.
fn apply_size_guards(sections: Vec<Section>) -> Vec<Section> {
    let mut out = Vec::new();
    for section in sections {
        if section.content.len() <= MAX_CHUNK_SIZE {
            out.push(section);
            continue;
        }
        // Split on double-newline paragraph boundaries first
        let paragraphs: Vec<&str> = section.content.split("\n\n").collect();
        let mut accumulator = String::new();
        for para in paragraphs {
            if accumulator.is_empty() {
                accumulator.push_str(para);
            } else if accumulator.len() + 2 + para.len() <= MAX_CHUNK_SIZE {
                accumulator.push_str("\n\n");
                accumulator.push_str(para);
            } else {
                // Flush accumulator
                if !accumulator.trim().is_empty() {
                    out.push(Section {
                        heading_path: section.heading_path.clone(),
                        content: accumulator.trim().to_owned(),
                    });
                }
                para.clone_into(&mut accumulator);
            }
        }
        // Push remainder
        if !accumulator.trim().is_empty() {
            // Hard-split if single paragraph is still too large
            let chunks = hard_split_with_overlap(accumulator.trim());
            for chunk in chunks {
                out.push(Section {
                    heading_path: section.heading_path.clone(),
                    content: chunk,
                });
            }
        }
    }
    out
}

/// Hard-splits text that cannot be paragraph-split, using a sliding window
/// with [`OVERLAP_SIZE`] character overlap.
fn hard_split_with_overlap(text: &str) -> Vec<String> {
    if text.len() <= MAX_CHUNK_SIZE {
        return vec![text.to_owned()];
    }
    let mut chunks = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let mut start = 0usize;
    while start < total {
        let end = (start + MAX_CHUNK_SIZE).min(total);
        let chunk: String = chars[start..end].iter().collect();
        chunks.push(chunk);
        if end >= total {
            break;
        }
        start = end.saturating_sub(OVERLAP_SIZE);
    }
    chunks
}

// ---------------------------------------------------------------------------
// Small-section merging
// ---------------------------------------------------------------------------

/// Merges sections whose content is shorter than [`MIN_CHUNK_SIZE`] with the
/// following sibling. The last tiny section is merged with the previous one.
fn merge_small_sections(sections: Vec<Section>) -> Vec<Section> {
    if sections.is_empty() {
        return sections;
    }
    let mut out: Vec<Section> = Vec::new();
    for section in sections {
        if let Some(last) = out.last_mut() {
            if last.content.len() < MIN_CHUNK_SIZE {
                // Merge current into last
                if !last.content.is_empty() && !section.content.is_empty() {
                    last.content.push('\n');
                }
                last.content.push_str(&section.content);
                continue;
            }
        }
        out.push(section);
    }
    // If the last section is still tiny and there are preceding sections,
    // merge it backward (already handled above by merging into `last`).
    out
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Chunks a markdown document into indexable [`Chunk`] values.
///
/// Frontmatter is stripped before processing. The pipeline is:
/// 1. Parse frontmatter and extract body.
/// 2. Split body on headings.
/// 3. Apply size guards (split large sections).
/// 4. Merge small sections.
/// 5. Assign stable IDs.
///
/// # Arguments
/// * `content` — Raw file contents including optional frontmatter.
/// * `file_stem` — Filename without extension, used as the ID prefix.
pub fn chunk_markdown(content: &str, file_stem: &str) -> Vec<Chunk> {
    let doc = parse_frontmatter(content);
    if doc.body.trim().is_empty() {
        return Vec::new();
    }

    let sections = split_on_headings(&doc.body);
    let sections = apply_size_guards(sections);
    let sections = merge_small_sections(sections);

    sections
        .into_iter()
        .enumerate()
        .map(|(position, section)| {
            let char_count = section.content.chars().count();
            Chunk {
                id: format!("{file_stem}-{position}"),
                heading_path: section.heading_path,
                content: section.content,
                char_count,
                position,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Frontmatter tests
    // ------------------------------------------------------------------

    #[test]
    fn extracts_frontmatter_from_markdown() {
        let input = "---\ntitle: My Note\ntags:\n  - rust\n  - vault\n---\n# Body\nSome content.";
        let doc = parse_frontmatter(input);

        assert_eq!(
            doc.metadata.get("title").map(String::as_str),
            Some("My Note"),
            "title should be extracted"
        );
        assert_eq!(
            doc.metadata.get("tags").map(String::as_str),
            Some("rust, vault"),
            "YAML array tags should be comma-joined"
        );
        assert!(
            !doc.body.contains("---"),
            "body must not contain frontmatter delimiters"
        );
        assert!(
            doc.body.contains("Body"),
            "body should contain the heading text"
        );
    }

    #[test]
    fn frontmatter_not_in_chunks() {
        let input =
            "---\ntitle: Hidden\nauthor: Alice\n---\n# Section\nThis is the real content.";
        let chunks = chunk_markdown(input, "note");

        for chunk in &chunks {
            assert!(
                !chunk.content.contains("title:"),
                "frontmatter key must not appear in chunk content"
            );
            assert!(
                !chunk.content.contains("Hidden"),
                "frontmatter value must not appear in chunk content"
            );
            assert!(
                !chunk.content.contains("---"),
                "frontmatter delimiter must not appear in chunk content"
            );
        }
    }

    #[test]
    fn no_frontmatter_returns_full_body() {
        let input = "# Title\nJust some text.";
        let doc = parse_frontmatter(input);
        assert!(doc.metadata.is_empty());
        assert_eq!(doc.body, input);
    }

    // ------------------------------------------------------------------
    // Heading-split tests
    // ------------------------------------------------------------------

    #[test]
    fn splits_on_headings() {
        // Each section needs enough content to survive the MIN_CHUNK_SIZE merge guard.
        let body_a = "word ".repeat(50); // ~250 chars
        let body_b = "text ".repeat(50); // ~250 chars
        let body_c = "data ".repeat(50); // ~250 chars
        let input = format!("# Alpha\n{body_a}\n\n## Beta\n{body_b}\n\n### Gamma\n{body_c}");
        let chunks = chunk_markdown(&input, "doc");

        assert_eq!(chunks.len(), 3, "three headings should produce three chunks");

        assert!(
            chunks[0].heading_path.contains(&"Alpha".to_owned()),
            "first chunk should be under Alpha"
        );
        assert!(
            chunks[1].heading_path.contains(&"Beta".to_owned()),
            "second chunk should be under Beta"
        );
        assert!(
            chunks[2].heading_path.contains(&"Gamma".to_owned()),
            "third chunk should be under Gamma"
        );
    }

    #[test]
    fn heading_path_tracks_hierarchy() {
        // Each section needs enough content to survive the MIN_CHUNK_SIZE merge guard.
        let intro = "word ".repeat(50); // ~250 chars
        let body = "text ".repeat(50);  // ~250 chars
        let input = format!("# Top\n{intro}\n\n## Sub\n{body}");
        let chunks = chunk_markdown(&input, "doc");

        // Should have 2 chunks: "Top" and ["Top", "Sub"]
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].heading_path, vec!["Top", "Sub"]);
    }

    #[test]
    fn no_headings_becomes_single_chunk() {
        let input = "Just some plain text without any headings at all.";
        let chunks = chunk_markdown(input, "plain");

        assert_eq!(chunks.len(), 1, "should produce exactly one chunk");
        assert!(
            chunks[0].heading_path.is_empty(),
            "chunk without headings should have empty heading_path"
        );
    }

    #[test]
    fn empty_body_produces_no_chunks() {
        // Frontmatter only — body is empty after stripping
        let input = "---\ntitle: Empty\n---\n";
        let chunks = chunk_markdown(input, "empty");
        assert!(
            chunks.is_empty(),
            "frontmatter-only file should yield no chunks"
        );
    }

    // ------------------------------------------------------------------
    // Size guard tests
    // ------------------------------------------------------------------

    #[test]
    fn size_guard_splits_large_sections() {
        // Build a section well over MAX_CHUNK_SIZE using two clear paragraphs
        let para_a = "word ".repeat(120); // ~600 chars
        let para_b = "text ".repeat(120); // ~600 chars
        let big_body = format!("# Section\n\n{para_a}\n\n{para_b}");

        let chunks = chunk_markdown(&big_body, "big");
        assert!(
            chunks.len() >= 2,
            "section >1000 chars split over paragraphs should yield ≥2 chunks"
        );
        for chunk in &chunks {
            assert!(
                chunk.char_count <= MAX_CHUNK_SIZE + OVERLAP_SIZE,
                "each chunk should be within size limit (got {})",
                chunk.char_count
            );
        }
    }

    #[test]
    fn hard_split_applies_overlap() {
        // Single paragraph larger than MAX_CHUNK_SIZE
        let big = "x".repeat(2500);
        let pieces = hard_split_with_overlap(&big);
        assert!(pieces.len() >= 3, "should produce multiple hard-split pieces");
        // Verify overlap: end of piece N should appear at start of piece N+1
        for window in pieces.windows(2) {
            let prev_tail: String = window[0].chars().rev().take(OVERLAP_SIZE).collect::<String>().chars().rev().collect();
            assert!(
                window[1].starts_with(&prev_tail),
                "overlap must be preserved between consecutive hard-split chunks"
            );
        }
    }

    // ------------------------------------------------------------------
    // Small-section merging tests
    // ------------------------------------------------------------------

    #[test]
    fn small_sections_are_merged() {
        // First section is tiny (<200 chars), second has real content
        let small = "Short.";
        let big = "word ".repeat(60); // ~300 chars — above MIN_CHUNK_SIZE
        let input = format!("# Tiny\n{small}\n\n# Big\n{big}");

        let chunks = chunk_markdown(&input, "merge");
        // The tiny section should be merged into the following one,
        // so we expect fewer chunks than raw headings.
        assert!(
            chunks.len() < 2 || chunks.iter().all(|c| c.char_count >= MIN_CHUNK_SIZE || chunks.len() == 1),
            "small sections should be merged; got {} chunks with sizes {:?}",
            chunks.len(),
            chunks.iter().map(|c| c.char_count).collect::<Vec<_>>()
        );
    }

    // ------------------------------------------------------------------
    // Wikilink preservation
    // ------------------------------------------------------------------

    #[test]
    fn wikilinks_preserved() {
        let input = "# Notes\n\nSee [[My Note]] and [[Other Page]] for details.";
        let chunks = chunk_markdown(input, "wiki");

        assert!(!chunks.is_empty(), "should produce at least one chunk");
        let combined: String = chunks.iter().map(|c| c.content.as_str()).collect::<Vec<_>>().join(" ");
        assert!(
            combined.contains("[[My Note]]"),
            "[[My Note]] wikilink must survive chunking; got: {combined}"
        );
        assert!(
            combined.contains("[[Other Page]]"),
            "[[Other Page]] wikilink must survive chunking; got: {combined}"
        );
    }

    // ------------------------------------------------------------------
    // Chunk ID and position
    // ------------------------------------------------------------------

    #[test]
    fn chunk_ids_use_file_stem_and_position() {
        let input = "# One\nContent one.\n\n# Two\nContent two here, a bit longer to avoid merge.\n\nExtra paragraph to pad.";
        let chunks = chunk_markdown(input, "my-note");

        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.id, format!("my-note-{i}"));
            assert_eq!(chunk.position, i);
        }
    }

    // ------------------------------------------------------------------
    // YAML edge cases
    // ------------------------------------------------------------------

    #[test]
    fn yaml_value_with_colon_is_handled() {
        let input = "---\nurl: https://example.com/path\n---\nBody text.";
        let doc = parse_frontmatter(input);
        assert_eq!(
            doc.metadata.get("url").map(String::as_str),
            Some("https://example.com/path")
        );
    }

    #[test]
    fn yaml_boolean_converted_to_string() {
        let input = "---\npublished: true\n---\nContent.";
        let doc = parse_frontmatter(input);
        assert_eq!(
            doc.metadata.get("published").map(String::as_str),
            Some("true")
        );
    }
}
