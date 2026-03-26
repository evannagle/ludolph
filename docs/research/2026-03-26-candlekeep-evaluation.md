# CandleKeep Evaluation for lu learn Integration

Date: 2026-03-26
Branch: feature/evaluate-candlekeeps-bookreading-approach
Status: Research complete

## What is CandleKeep?

A Python RAG knowledge base server ([GitHub](https://github.com/BansheeEmperor/candlekeep)) that provides semantic search and document management via MCP. Uses ChromaDB for vector storage.

- **Language:** Python 3.10+
- **License:** GPL-3.0
- **Version:** 2.0.0
- **Maturity:** Small project (7 stars, 2 forks, created 2026-02-13)

## Ingestion Pipeline

### Supported Formats

`.txt`, `.md`, `.pdf`, `.rst`, `.json`, `.yaml`, `.yml`
No EPUB support. PDF extraction via `pymupdf4llm` (converts to markdown) with `pdfplumber` fallback.

### Chunking Strategy

1. **Header-aware splitting** - Detects ATX headers (`# ...`) and Setext headers. Splits on markdown section boundaries first.
2. **Fixed-size fallback** - If no headers or section exceeds chunk_size, falls back to fixed-size chunking.
3. **Defaults:** chunk_size=512 chars, chunk_overlap=50 chars (benchmarked against 256/768/1024).
4. **Bardic Knowledge** - Document title + description from YAML frontmatter prepended to every chunk before embedding. Bakes document context into vectors at ingestion time.
5. **True Sight** - Images in PDFs/markdown are captioned via a VLM at ingestion, creating searchable text from diagrams.

### Quality Gate

Documents must have YAML frontmatter (title, description) and markdown header structure. Ensures Bardic Knowledge has metadata and chunks split on meaningful boundaries.

## Search Architecture

Three search paths with adaptive routing:

| Path | Technique | Latency | Use Case |
|------|-----------|---------|----------|
| `hybrid` (default) | BM25 + Vector via RRF | ~48ms | General queries |
| `precise` | Vector + cross-encoder reranking | ~921ms | Complex comparisons |
| `explore` | Entity expansion | varies | Discovery, relationships |

Notable techniques:
- **Arcane Recall** - Expands results by +/-2 adjacent chunks using similarity-weighted expansion. +17% content match.
- **Relevance Ward** - Score thresholds filter irrelevant results. Returns "I don't know" rather than garbage.
- **Rosetta Seal** - Corpus-derived BM25 normalisation bridging surface-form variants. +15.7% BM25 MRR.

### Embedding Model

`BAAI/bge-small-en-v1.5` (default). LLM providers pluggable: Anthropic, OpenAI, Bedrock, Ollama.

### Dependencies (Heavy)

`fastmcp`, `chromadb`, `sentence-transformers`, `spacy`, `pdfplumber`, `rank-bm25`, `numpy`.
Optional: `ragatouille` (ColBERT), `boto3` (Bedrock).

## Comparison: CandleKeep vs Ludolph's Needs

### What Ludolph has today

- Read-only sandboxed vault access (read_file, search, list_directory)
- Rust codebase, runs on Raspberry Pi
- No document ingestion, no embeddings, no RAG pipeline
- No `lu learn` command exists yet

### Alignment

| Aspect | CandleKeep | Ludolph Need | Match? |
|--------|-----------|--------------|--------|
| Language | Python | Rust | No |
| Runtime | Server w/ ChromaDB | Pi-friendly, minimal deps | No |
| Formats | PDF, MD, TXT | Books (PDF, EPUB) | Partial |
| Chunking | Header-aware + fixed | Book-structure-aware | Partial |
| Search | Full RAG (BM25 + vector) | Vault search exists; need book search | Relevant |
| MCP | Yes (fastmcp) | Yes (custom Rust) | Protocol match |
| Resource footprint | Heavy (spacy, chromadb, sentence-transformers) | Must run on Pi | No |
| License | GPL-3.0 | Would require GPL compliance | Concern |

### Key Gaps

1. **No EPUB support** - CandleKeep doesn't handle EPUBs, which are a primary book format.
2. **Resource requirements** - ChromaDB + spacy + sentence-transformers won't run comfortably on a Raspberry Pi.
3. **Language mismatch** - Ludolph is Rust; CandleKeep is Python. Integration means either running a Python sidecar or porting logic.
4. **GPL-3.0 license** - Forking would require Ludolph to adopt GPL, which constrains future licensing.
5. **Overkill for initial scope** - CandleKeep's full RAG pipeline (hybrid search, reranking, entity expansion) is sophisticated but more than what `lu learn` needs initially.

### What's Worth Borrowing

1. **Header-aware chunking** - Splitting on markdown sections before falling back to fixed-size is a sound strategy. Simple to implement in Rust.
2. **Bardic Knowledge pattern** - Prepending document metadata to each chunk improves retrieval quality. Easy to adopt.
3. **PDF-to-markdown conversion** - The approach of converting PDFs to markdown first, then chunking markdown, is cleaner than trying to chunk raw PDF content.
4. **Quality gate concept** - Requiring frontmatter/structure before ingestion prevents garbage-in problems.
5. **Adjacent chunk expansion** - Returning neighboring chunks around matches provides better context. Worth implementing later.

## Decision: Build Custom

**Recommendation: Build a custom Rust-native `lu learn` pipeline, borrowing CandleKeep's design patterns.**

### Rationale

1. **CandleKeep can't run on Pi** - The dependency stack (ChromaDB, spacy, sentence-transformers) is too heavy. This alone rules out direct integration or forking.

2. **Language mismatch makes forking impractical** - Porting Python to Rust is more work than building from scratch with the right architecture in mind.

3. **GPL-3.0 is a licensing constraint** - Even if we could use it, GPL would propagate to Ludolph.

4. **The valuable ideas are patterns, not code** - The chunking strategy, metadata prepending, and PDF-to-markdown pipeline are design patterns that can be implemented cleanly in Rust without needing CandleKeep's code.

### Proposed lu learn Architecture (Sketch)

```
lu learn <file>
  │
  ├─ Format Detection (.pdf, .epub, .md, .txt)
  │
  ├─ Conversion to Markdown
  │   ├─ PDF: pdf-extract or lopdf crate
  │   ├─ EPUB: epub crate → extract chapters as markdown
  │   └─ MD/TXT: passthrough
  │
  ├─ Chunking Pipeline
  │   ├─ Extract frontmatter/metadata
  │   ├─ Header-aware splitting (borrowed from CandleKeep)
  │   ├─ Fixed-size fallback for long sections
  │   └─ Prepend document metadata to each chunk (Bardic Knowledge pattern)
  │
  ├─ Storage
  │   ├─ Chunks stored as markdown files in vault (e.g., vault/learned/<book>/)
  │   └─ Index file with metadata, chunk map, reading progress
  │
  └─ Search Integration
      └─ Existing vault search tools work on learned content automatically
```

### Key Design Choices

- **No vector database initially** - Store chunks as plain files in the vault. The existing search tools (ripgrep-based) work immediately. Add embeddings later if needed.
- **Rust-native PDF/EPUB parsing** - Use `pdf-extract` or `lopdf` for PDFs, `epub` crate for EPUBs. Keeps the single-binary deployment model.
- **Vault-native storage** - Learned content lives in the vault alongside notes. No separate database to manage on Pi.
- **Progressive enhancement** - Start with basic chunking, add metadata prepending, then consider embeddings as a future phase.

### Crates to Evaluate

| Purpose | Crate | Notes |
|---------|-------|-------|
| PDF text extraction | `pdf-extract` | Pure Rust, no C deps |
| PDF parsing | `lopdf` | Low-level PDF manipulation |
| EPUB parsing | `epub` | Extract chapters/metadata |
| Markdown parsing | `pulldown-cmark` | Already well-known in Rust ecosystem |

## Next Steps

1. Prototype the chunking pipeline in Rust with header-aware splitting
2. Evaluate `pdf-extract` and `epub` crates for format support
3. Design the vault storage format for learned content
4. Implement basic `lu learn <file>` CLI command
5. Test with a real book PDF and EPUB
