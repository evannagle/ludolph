#![allow(unused_imports)] // Module is built incrementally; re-exports used in Task 4 (Indexer)

pub mod chunker;
pub mod enricher;
pub mod indexer;
pub mod manifest;
pub mod watcher;

pub use chunker::{chunk_markdown, parse_frontmatter, Chunk, ParsedDocument};
pub use indexer::Indexer;
pub use manifest::Manifest;
