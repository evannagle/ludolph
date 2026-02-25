"""Search operations module."""

import re

from ..security import get_vault_path, safe_path

TOOLS = [
    {
        "name": "search",
        "description": "Search for files or content (simple text search)",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (searches file names and content)",
                },
                "path": {"type": "string", "description": "Optional subdirectory to search within"},
                "context_length": {
                    "type": "integer",
                    "description": "Number of characters of context around matches (default 50)",
                },
            },
            "required": ["query"],
        },
    },
    {
        "name": "search_advanced",
        "description": "Advanced search with regex and glob patterns",
        "input_schema": {
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regex pattern to search for"},
                "path": {"type": "string", "description": "Optional subdirectory to search within"},
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g., '*.md')",
                },
                "content_only": {
                    "type": "boolean",
                    "description": "Search only file content, not names",
                },
            },
            "required": ["pattern"],
        },
    },
]


def _search(args: dict) -> dict:
    """Simple text search across file names and content."""
    query = args.get("query", "")
    if not query:
        return {"content": "", "error": "Query required"}

    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    context_length = args.get("context_length", 50)

    results = []
    pattern = re.compile(re.escape(query), re.IGNORECASE)

    for path in search_path.rglob("*"):
        if path.is_file() and not any(p.startswith(".") for p in path.parts):
            rel_path = path.relative_to(get_vault_path())

            # Check filename match
            if pattern.search(path.name):
                results.append(f"file: {rel_path}")

            # Check content for text files
            elif path.suffix in (
                ".md",
                ".txt",
                ".json",
                ".yaml",
                ".yml",
                ".py",
                ".js",
                ".ts",
                ".rs",
            ):
                try:
                    content = path.read_text(encoding="utf-8")
                    match = pattern.search(content)
                    if match:
                        # Extract context around match
                        start = max(0, match.start() - context_length)
                        end = min(len(content), match.end() + context_length)
                        context = content[start:end].replace("\n", " ")
                        results.append(f"match: {rel_path}\n  ...{context}...")
                except Exception:
                    pass

        if len(results) >= 50:
            break

    content = "\n".join(results) if results else "No matches found"
    return {"content": content, "error": None}


def _search_advanced(args: dict) -> dict:
    """Advanced search with regex and glob patterns."""
    pattern_str = args.get("pattern", "")
    if not pattern_str:
        return {"content": "", "error": "Pattern required"}

    try:
        pattern = re.compile(pattern_str, re.IGNORECASE | re.MULTILINE)
    except re.error as e:
        return {"content": "", "error": f"Invalid regex: {e}"}

    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    glob_pattern = args.get("glob", "*")
    content_only = args.get("content_only", False)

    results = []

    for path in search_path.rglob(glob_pattern):
        if path.is_file() and not any(p.startswith(".") for p in path.parts):
            rel_path = path.relative_to(get_vault_path())

            # Check filename match (unless content_only)
            if not content_only and pattern.search(path.name):
                results.append(f"file: {rel_path}")

            # Check content
            try:
                content = path.read_text(encoding="utf-8")
                matches = list(pattern.finditer(content))
                if matches:
                    for match in matches[:3]:  # Limit matches per file
                        start = max(0, match.start() - 30)
                        end = min(len(content), match.end() + 30)
                        context = content[start:end].replace("\n", " ")
                        results.append(f"match: {rel_path}:{match.start()}\n  ...{context}...")
            except Exception:
                pass

        if len(results) >= 50:
            break

    content = "\n".join(results) if results else "No matches found"
    return {"content": content, "error": None}


HANDLERS = {
    "search": _search,
    "search_advanced": _search_advanced,
}
