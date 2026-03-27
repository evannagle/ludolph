use regex::RegexBuilder;
use serde_json::{Value, json};
use std::path::Path;
use walkdir::WalkDir;

use super::Tool;
use crate::index::indexer::{ChunkFile, StoredChunk};
use crate::index::manifest::Manifest;

pub fn definition() -> Tool {
    Tool {
        name: "search_index".to_string(),
        description:
            "Search the vault index for matching chunks. Returns relevant sections with source \
             files and heading context. More targeted than full-text search."
                .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (case-insensitive, supports regex)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default 10)"
                }
            },
            "required": ["query"]
        }),
    }
}

pub fn execute(input: &Value, vault_path: &Path) -> String {
    let Some(query) = input.get("query").and_then(|v| v.as_str()) else {
        return "Error: 'query' parameter is required".to_string();
    };

    let max_results = input
        .get("max_results")
        .and_then(Value::as_u64)
        .map_or(10, |n| usize::try_from(n).unwrap_or(10));

    let chunks_dir = Manifest::chunks_dir(vault_path);
    if !chunks_dir.exists() {
        return "Index not found. Run `lu index` to build it.".to_string();
    }

    let pattern = match RegexBuilder::new(query).case_insensitive(true).build() {
        Ok(p) => p,
        Err(_) => match RegexBuilder::new(&regex::escape(query))
            .case_insensitive(true)
            .build()
        {
            Ok(p) => p,
            Err(e) => return format!("Error building search pattern: {e}"),
        },
    };

    let mut results: Vec<ScoredResult> = Vec::new();

    for entry in WalkDir::new(&chunks_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let Ok(json_text) = std::fs::read_to_string(path) else {
            continue;
        };

        let Ok(chunk_file) = serde_json::from_str::<ChunkFile>(&json_text) else {
            continue;
        };

        for chunk in &chunk_file.chunks {
            if let Some(scored) = score_chunk(&chunk_file.source, chunk, &pattern) {
                results.push(scored);
            }
        }
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(max_results);

    if results.is_empty() {
        return format!("No matches found for '{query}' in the index.");
    }

    results
        .into_iter()
        .map(|r| {
            let heading = if r.heading_path.is_empty() {
                String::new()
            } else {
                format!(" > {}", r.heading_path.join(" > "))
            };
            let preview: String = r.content.chars().take(300).collect();
            let summary_line = r
                .summary
                .as_deref()
                .map(|s| format!("Summary: {s}\n"))
                .unwrap_or_default();
            format!(
                "--- {}{} ---\n{}{}\n",
                r.source, heading, summary_line, preview
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct ScoredResult {
    score: f64,
    source: String,
    heading_path: Vec<String>,
    summary: Option<String>,
    content: String,
}

fn score_chunk(source: &str, chunk: &StoredChunk, pattern: &regex::Regex) -> Option<ScoredResult> {
    let summary_match = chunk
        .summary
        .as_deref()
        .is_some_and(|s| pattern.is_match(s));
    let content_match = pattern.is_match(&chunk.content);

    if !summary_match && !content_match {
        return None;
    }

    let mut score = 0.0_f64;

    if summary_match {
        score += 2.0;
    }
    if content_match {
        score += 1.0;
    }

    // Signal density bonus: shorter chunks get a small boost
    #[allow(clippy::cast_precision_loss)]
    let density_bonus = 1.0 / (chunk.char_count as f64 / 100.0).max(1.0) * 0.5;
    score += density_bonus;

    // Position 0 boost
    if chunk.position == 0 {
        score += 0.3;
    }

    Some(ScoredResult {
        score,
        source: source.to_string(),
        heading_path: chunk.heading_path.clone(),
        summary: chunk.summary.clone(),
        content: chunk.content.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::IndexTier;
    use crate::index::indexer::ChunkFile;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    fn make_vault() -> TempDir {
        tempfile::tempdir().expect("Failed to create temp dir")
    }

    fn write_chunk_file(chunks_dir: &Path, filename: &str, chunk_file: &ChunkFile) {
        let path = chunks_dir.join(filename);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let json = serde_json::to_string_pretty(chunk_file).unwrap();
        fs::write(path, json).unwrap();
    }

    fn make_chunk_file(source: &str, content: &str, summary: Option<&str>) -> ChunkFile {
        ChunkFile {
            source: source.to_string(),
            source_hash: "abc123".to_string(),
            indexed_at: "2024-01-01T00:00:00Z".to_string(),
            tier: IndexTier::Standard.to_string(),
            frontmatter: HashMap::new(),
            chunks: vec![crate::index::indexer::StoredChunk {
                id: "chunk-0".to_string(),
                heading_path: vec!["Notes".to_string()],
                content: content.to_string(),
                summary: summary.map(str::to_string),
                char_count: content.len(),
                position: 0,
            }],
        }
    }

    #[test]
    fn finds_matching_chunks() {
        let vault = make_vault();
        let chunks_dir = Manifest::chunks_dir(vault.path());
        fs::create_dir_all(&chunks_dir).unwrap();

        let cf = make_chunk_file("notes/hello.md", "This is about Rust programming.", None);
        write_chunk_file(&chunks_dir, "notes/hello.json", &cf);

        let input = json!({ "query": "rust" });
        let result = execute(&input, vault.path());

        assert!(result.contains("notes/hello.md"), "Should include source file");
        assert!(result.contains("Rust programming"), "Should include content preview");
    }

    #[test]
    fn returns_empty_for_no_matches() {
        let vault = make_vault();
        let chunks_dir = Manifest::chunks_dir(vault.path());
        fs::create_dir_all(&chunks_dir).unwrap();

        let cf = make_chunk_file("notes/hello.md", "This is about Rust programming.", None);
        write_chunk_file(&chunks_dir, "notes/hello.json", &cf);

        let input = json!({ "query": "python" });
        let result = execute(&input, vault.path());

        assert!(
            result.contains("No matches found for 'python'"),
            "Should report no matches"
        );
    }

    #[test]
    fn ranks_summary_matches_higher() {
        let vault = make_vault();
        let chunks_dir = Manifest::chunks_dir(vault.path());
        fs::create_dir_all(&chunks_dir).unwrap();

        // Chunk A: query only in content
        let cf_a = make_chunk_file("notes/a.md", "Rust is a systems programming language.", None);
        write_chunk_file(&chunks_dir, "notes/a.json", &cf_a);

        // Chunk B: query in summary (should rank higher)
        let cf_b = make_chunk_file(
            "notes/b.md",
            "Some other content here with no direct keyword.",
            Some("Rust programming overview"),
        );
        write_chunk_file(&chunks_dir, "notes/b.json", &cf_b);

        let input = json!({ "query": "rust", "max_results": 10 });
        let result = execute(&input, vault.path());

        // b.md should appear before a.md because its summary matches
        let pos_a = result.find("notes/a.md").unwrap_or(usize::MAX);
        let pos_b = result.find("notes/b.md").unwrap_or(usize::MAX);
        assert!(
            pos_b < pos_a,
            "Summary match (b.md) should rank higher than content-only match (a.md)"
        );
    }

    #[test]
    fn returns_guidance_when_no_index() {
        let vault = make_vault();
        // No chunks dir created

        let input = json!({ "query": "anything" });
        let result = execute(&input, vault.path());

        assert!(
            result.contains("lu index"),
            "Should guide user to run `lu index`"
        );
    }

    #[test]
    fn truncates_to_max_results() {
        let vault = make_vault();
        let chunks_dir = Manifest::chunks_dir(vault.path());
        fs::create_dir_all(&chunks_dir).unwrap();

        // Write 5 chunk files all matching
        for i in 0..5 {
            let cf = make_chunk_file(
                &format!("notes/file{i}.md"),
                "Rust is great for systems programming.",
                None,
            );
            write_chunk_file(&chunks_dir, &format!("notes/file{i}.json"), &cf);
        }

        let input = json!({ "query": "rust", "max_results": 2 });
        let result = execute(&input, vault.path());

        let count = result.matches("--- ").count();
        assert_eq!(count, 2, "Should only return max_results results");
    }
}
