# Learning and Teaching

Lu starts with your vault. But sometimes you need Lu to know things that aren't in your vault — a codebase you're working on, documentation for an API, a PDF you downloaded, an article you read. The learn/teach pipeline lets Lu ingest anything and use it alongside your vault.

## Learning

### Files

```bash
lu learn ~/Downloads/whitepaper.pdf
lu learn ~/Research/notes.md
```

Lu chunks the file, generates embeddings, and stores them persistently. Next time you ask about something in that file, Lu finds it.

PDF support requires `pymupdf` (`pip install pymupdf`). Text, markdown, RST, TOML, YAML, JSON, and CSV files work out of the box.

### URLs

```bash
lu learn https://docs.example.com/api-reference
```

Lu fetches the page, strips the HTML, and ingests the text. Good for documentation, articles, blog posts. Not great for JavaScript-heavy single-page apps (there's no browser rendering).

### Folders

```bash
lu learn ~/Research/papers/
```

Lu walks the directory and ingests every supported file. Hidden files and directories are skipped.

### GitHub repos

```bash
lu learn github:evannagle/ludolph
```

Lu shallow-clones the repo, indexes code and docs, and stores it all in a namespaced knowledge base. Skips `node_modules`, `target`, `.git`, and other noise.

### What gets stored

Everything Lu learns goes into an embedding store at `~/.ludolph/embeddings.db`. Each source gets its own namespace:

| Source | Namespace |
|--------|-----------|
| Vault chunks | `vault` |
| Files | `learned/files` |
| URLs | `learned/urls` |
| GitHub repos | `learned/github/owner/repo` |

This means vault content and learned content don't collide. When Lu searches, it searches across all namespaces by default.

### Checking status

```bash
lu learn anything --status
```

Or via Telegram, ask Lu: "What have you learned?"

### Forgetting

```bash
lu learn myfile.pdf --forget
lu learn github:company/old-project --forget
lu learn all --forget           # forget everything learned (vault is untouched)
```

Clean removal. The vault index is never affected.

## Teaching

Teaching is the other direction: Lu takes what it knows and packages it for an audience. This works with vault content, learned content, or both.

### Audiences

```bash
lu teach "authentication patterns"                # default: plain language
lu teach "authentication patterns" -f coders      # code examples, technical depth
lu teach "authentication patterns" -f robots      # structured JSON, machine-readable
lu teach "authentication patterns" -f "my manager"  # custom audience
```

Lu retrieves relevant chunks from the embedding store, then asks Claude to synthesize an explanation tailored to the audience. The result includes source citations.

### Exporting

```bash
lu teach "Rust async" --export               # tier 2 (structure, no full text)
lu teach "Rust async" --export --tier 3      # tier 3 (full text included)
lu teach "Rust async" --export --tier 1      # tier 1 (metadata only)
```

Exports produce `.ludo` packages — JSON files with three privacy tiers:

| Tier | Includes | Use case |
|------|----------|----------|
| 1 | Topic, source names, chunk count | "What do you know about this?" |
| 2 | Headings, structure, no text | "How is this organized?" |
| 3 | Full text | "Teach me everything" |

## Observations

Separate from learn/teach, Lu has an observations system for remembering things about you. This isn't file ingestion — it's conversational memory.

Tell Lu:
- "Remember that I prefer morning briefs without newsletters"
- "I'm working on a book about a philosopher named Karl"
- "My timezone is Pacific"

Lu saves these as observations with categories:
- **preference** — likes, defaults, style choices
- **fact** — biographical, family, work details
- **context** — active projects, goals, deadlines

Observations are loaded into Lu's system prompt on every conversation. They persist across sessions. You don't need to repeat yourself.

To check what Lu remembers, ask: "What do you know about me?"

## How it fits together

```
Your vault              Learned content         Observations
    │                        │                       │
    └──── Embedding Store ───┘                       │
              │                                      │
         lu search / lu teach                   System prompt
              │                                      │
              └──────────────────────────────────────┘
                              │
                         Lu's response
```

Lu searches your vault and learned content for relevant knowledge, while observations provide context about who you are and how you work. Both inform every response.

## Vault index

Before Lu can search your vault semantically, you need to build the index:

```bash
lu index                    # standard tier (chunking + search)
lu index --tier deep        # adds AI-generated summaries per chunk
lu index --status           # check index health
lu index --rebuild          # full rebuild from scratch
```

The index is incremental — a file watcher detects changes and re-chunks only what's new or modified. Staleness detection uses xxh3 hashing.

When `rebuild_semantic_index` is called (via Telegram or the CLI), it syncs the embedding store with the chunk files, only re-embedding chunks whose source hash has changed. This means vault re-indexing is fast after the first run.

## Publishing

If you want others to know what your vault can teach, publish a profile:

```bash
lu publish
```

This generates a vault profile showing your topics, knowledge stats, and sample queries — without exposing any content. The profile is published to the [community registry](https://github.com/evannagle/ludolph-registry) via a GitHub PR.

Published profiles appear on [ludolph.dev](https://ludolph.dev), where others can browse what vaults know and request to learn from them.
