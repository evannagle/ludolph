"""Vault index tools — search pre-chunked content and view vault structure."""

import json
import re
from pathlib import Path

from security import get_vault_path

TOOLS = [
    {
        "name": "search_index",
        "description": (
            "Search the vault index for matching chunks. Returns relevant sections "
            "with source files and heading context. More targeted than full-text "
            "search — finds pre-chunked sections of notes with optional AI-generated "
            "summaries."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (text or regex pattern)",
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results to return (default: 10)",
                },
            },
            "required": ["query"],
        },
    },
    {
        "name": "vault_map",
        "description": (
            "Get a high-level overview of the vault: structure, folder breakdown, "
            "index status, and statistics. Use this to understand the vault's layout "
            "before diving into specific files."
        ),
        "input_schema": {
            "type": "object",
            "properties": {},
            "required": [],
        },
    },
]


def _get_index_dir() -> Path:
    """Return the vault index directory."""
    return get_vault_path() / ".ludolph" / "index"


def _get_chunks_dir() -> Path:
    """Return the chunks directory."""
    return _get_index_dir() / "chunks"


def _search_index(args: dict) -> dict:
    """Search pre-chunked vault content with ranking."""
    query = args.get("query", "")
    if not query:
        return {"content": "", "error": "Query required"}

    max_results = args.get("max_results", 10)
    chunks_dir = _get_chunks_dir()

    if not chunks_dir.exists():
        return {
            "content": (
                "Index not found. Run `lu index` to build it.\n"
                "If index is at Quick tier, upgrade with `lu index --tier standard`."
            ),
            "error": None,
        }

    # Build regex, fall back to literal
    try:
        pattern = re.compile(query, re.IGNORECASE)
    except re.error:
        pattern = re.compile(re.escape(query), re.IGNORECASE)

    scored = []

    for json_path in chunks_dir.rglob("*.json"):
        try:
            chunk_file = json.loads(json_path.read_text(encoding="utf-8"))
        except Exception:
            continue

        source = chunk_file.get("source", "")

        for chunk in chunk_file.get("chunks", []):
            score = 0.0
            content = chunk.get("content", "")
            summary = chunk.get("summary")
            char_count = chunk.get("char_count", len(content))
            position = chunk.get("position", 1)

            # Summary match scores higher
            if summary and pattern.search(summary):
                score += 2.0

            # Content match
            if pattern.search(content):
                score += 1.0

            if score > 0:
                # Signal density bonus
                density = 1.0 / max(char_count / 100.0, 1.0)
                score += density * 0.5

                # Position 0 boost
                if position == 0:
                    score += 0.3

                scored.append({
                    "source": source,
                    "heading_path": chunk.get("heading_path", []),
                    "content": content,
                    "summary": summary,
                    "score": score,
                })

    if not scored:
        return {"content": f"No matches found for '{query}' in the index.", "error": None}

    scored.sort(key=lambda x: x["score"], reverse=True)
    scored = scored[:max_results]

    lines = [f"Found {len(scored)} matches for '{query}':\n"]
    for item in scored:
        heading = ""
        if item["heading_path"]:
            heading = f" > {' > '.join(item['heading_path'])}"
        lines.append(f"--- {item['source']}{heading} ---")
        if item["summary"]:
            lines.append(f"Summary: {item['summary']}")
        preview = item["content"][:300]
        if len(item["content"]) > 300:
            preview += "..."
        lines.append(preview)
        lines.append("")

    return {"content": "\n".join(lines), "error": None}


def _vault_map(args: dict) -> dict:
    """Return vault index overview."""
    index_dir = _get_index_dir()
    manifest_path = index_dir / "manifest.json"

    if not manifest_path.exists():
        return {
            "content": (
                "No vault index found. Run `lu index` to build one.\n"
                "Available tiers:\n"
                "- quick: file map only (free, seconds)\n"
                "- standard: chunked index (free, minutes)\n"
                "- deep: chunked + AI summaries (costs API tokens, hours)"
            ),
            "error": None,
        }

    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except Exception as e:
        return {"content": "", "error": f"Failed to read manifest: {e}"}

    lines = [
        f"Vault: {manifest.get('vault_path', 'unknown')}",
        f"Index tier: {manifest.get('tier', 'unknown')}",
        f"Files: {manifest.get('file_count', 0)}",
        f"Chunks: {manifest.get('chunk_count', 0)}",
        f"Last indexed: {manifest.get('last_indexed', 'unknown')}",
    ]

    folders = manifest.get("folders", {})
    if folders:
        lines.append("\nFolders:")
        sorted_folders = sorted(folders.items(), key=lambda x: x[1].get("file_count", 0), reverse=True)
        for folder, stats in sorted_folders:
            lines.append(f"  {folder}: {stats.get('file_count', 0)} files, {stats.get('chunk_count', 0)} chunks")

    return {"content": "\n".join(lines), "error": None}


HANDLERS = {
    "search_index": _search_index,
    "vault_map": _vault_map,
}
