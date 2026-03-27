//! Enricher — adds AI-generated summaries to Deep tier chunks via Claude Haiku.

use anyhow::Result;
use futures::future::join_all;
use serde::{Deserialize, Serialize};

use crate::index::indexer::StoredChunk;

/// Number of chunks to enrich concurrently in each batch.
const BATCH_SIZE: usize = 10;

/// Claude Haiku model identifier for enrichment.
const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";

/// Maximum tokens for enrichment summaries.
const MAX_TOKENS: u32 = 100;

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build the enrichment prompt for a chunk's content.
///
/// Returns a prompt that asks Claude Haiku to summarize the chunk and identify
/// what concepts and search terms it captures.
#[must_use]
pub fn build_enrichment_prompt(content: &str) -> String {
    format!(
        "Summarize this chunk in 1-2 sentences. What concept does it capture? \
What would someone search for to find this?\n\n{content}"
    )
}

/// Enrich a batch of chunks with AI-generated summaries using Claude Haiku.
///
/// Processes chunks in concurrent batches of [`BATCH_SIZE`]. Each chunk is sent
/// to the Claude Haiku API; successes populate `chunk.summary`. Failures are
/// logged as warnings and do not abort processing.
///
/// Returns the count of successfully enriched chunks.
pub async fn enrich_batch(chunks: &mut [StoredChunk], api_key: &str) -> Result<usize> {
    let mut enriched_count = 0usize;

    for batch_start in (0..chunks.len()).step_by(BATCH_SIZE) {
        let batch_end = (batch_start + BATCH_SIZE).min(chunks.len());
        let batch = &chunks[batch_start..batch_end];

        // Build concurrent futures for this batch.
        let futures: Vec<_> = batch
            .iter()
            .map(|chunk| call_haiku(&chunk.content, api_key))
            .collect();

        let results = join_all(futures).await;

        // Apply results back to the mutable slice.
        for (offset, result) in results.into_iter().enumerate() {
            let chunk = &mut chunks[batch_start + offset];
            match result {
                Some(summary) => {
                    chunk.summary = Some(summary);
                    enriched_count += 1;
                }
                None => {
                    tracing::warn!(
                        chunk_id = %chunk.id,
                        "Failed to enrich chunk; skipping"
                    );
                }
            }
        }
    }

    Ok(enriched_count)
}

// ---------------------------------------------------------------------------
// Private — API call
// ---------------------------------------------------------------------------

/// Call Claude Haiku to generate a summary for `content`.
///
/// Returns `Some(summary)` on success, `None` on any failure (network error,
/// bad response, empty content block).
async fn call_haiku(content: &str, api_key: &str) -> Option<String> {
    let prompt = build_enrichment_prompt(content);

    let request = ApiRequest {
        model: HAIKU_MODEL.to_owned(),
        max_tokens: MAX_TOKENS,
        messages: vec![Message {
            role: "user".to_owned(),
            content: prompt,
        }],
    };

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&request)
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        tracing::warn!(
            status = %response.status(),
            "Haiku API returned non-success status"
        );
        return None;
    }

    let api_response: ApiResponse = response.json().await.ok()?;
    api_response.content.into_iter().next().map(|block| block.text)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_enrichment_prompt() {
        let content = "This is some chunk content about Rust ownership.";
        let prompt = build_enrichment_prompt(content);

        // Must include the instruction text.
        assert!(
            prompt.contains("Summarize this chunk in 1-2 sentences"),
            "Prompt should include the summarize instruction"
        );

        // Must include the original content.
        assert!(
            prompt.contains(content),
            "Prompt should include the original chunk content"
        );
    }
}
