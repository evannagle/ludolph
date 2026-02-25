"""Backlinks and recent files operations module."""

import re
from datetime import datetime

from ..security import get_vault_path, safe_path

TOOLS = [
    {
        "name": "get_backlinks",
        "description": "Find all files that link to a specific file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the target file"},
                "include_context": {
                    "type": "boolean",
                    "description": "Include text around each link (default true)",
                },
            },
            "required": ["path"],
        },
    },
    {
        "name": "recent_files",
        "description": "List recently modified files",
        "input_schema": {
            "type": "object",
            "properties": {
                "days": {
                    "type": "integer",
                    "description": "Only files modified within this many days",
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of files to return (default 20)",
                },
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory to search within",
                },
            },
            "required": [],
        },
    },
]


def _get_backlinks(args: dict) -> dict:
    """Find all files that link to a specific file."""
    target_path = args.get("path", "")
    include_context = args.get("include_context", True)

    if not target_path:
        return {"content": "", "error": "Path required"}

    # Extract filename without extension for wikilink matching
    target_name = target_path.split("/")[-1]
    if target_name.endswith(".md"):
        target_name = target_name[:-3]

    # Build pattern to match wikilinks to this file
    # Matches [[target]] or [[target|alias]]
    pattern = re.compile(
        rf"\[\[{re.escape(target_name)}(?:\|[^\]]+)?\]\]", re.IGNORECASE
    )

    vault = get_vault_path()
    results = []

    for file_path in vault.rglob("*.md"):
        if any(p.startswith(".") for p in file_path.parts):
            continue

        try:
            content = file_path.read_text(encoding="utf-8")
            matches = list(pattern.finditer(content))

            if matches:
                rel_path = file_path.relative_to(vault)

                if include_context:
                    # Extract context around each match (limit to 3)
                    contexts = []
                    for match in matches[:3]:
                        start = max(0, match.start() - 40)
                        end = min(len(content), match.end() + 40)
                        ctx = content[start:end].replace("\n", " ")
                        contexts.append(f"  ...{ctx}...")

                    results.append(f"{rel_path}:\n" + "\n".join(contexts))
                else:
                    results.append(str(rel_path))

        except Exception:
            pass

    if not results:
        return {"content": f"No files link to {target_path}", "error": None}

    return {"content": "\n\n".join(results), "error": None}


def _recent_files(args: dict) -> dict:
    """List recently modified files."""
    days = args.get("days")
    limit = args.get("limit", 20)

    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    # Calculate cutoff timestamp if days specified
    cutoff = None
    if days:
        cutoff = datetime.now().timestamp() - (days * 86400)

    files_with_mtime = []

    for file_path in search_path.rglob("*"):
        if not file_path.is_file():
            continue
        if any(p.startswith(".") for p in file_path.parts):
            continue

        try:
            mtime = file_path.stat().st_mtime

            if cutoff and mtime < cutoff:
                continue

            rel_path = file_path.relative_to(get_vault_path())
            files_with_mtime.append((mtime, rel_path))
        except Exception:
            pass

    # Sort by modification time, newest first
    files_with_mtime.sort(reverse=True)

    # Apply limit
    files_with_mtime = files_with_mtime[:limit]

    if not files_with_mtime:
        return {"content": "(no recent files found)", "error": None}

    lines = []
    for mtime, rel_path in files_with_mtime:
        timestamp = datetime.fromtimestamp(mtime).strftime("%Y-%m-%d %H:%M")
        lines.append(f"{timestamp} {rel_path}")

    return {"content": "\n".join(lines), "error": None}


HANDLERS = {
    "get_backlinks": _get_backlinks,
    "recent_files": _recent_files,
}
