"""Tag management operations module."""

import re
from collections import Counter

from ..security import get_vault_path, safe_path

# Pattern for hashtags, avoiding matches inside code or words
# Negative lookbehind prevents matching inside words or after backticks
TAG_PATTERN = re.compile(r"(?<![`\w])#([\w\-/]+)", re.UNICODE)

# Patterns for removing code blocks before tag extraction
FENCED_CODE_RE = re.compile(r"```.*?```", re.DOTALL)
INLINE_CODE_RE = re.compile(r"`[^`]+`")

TOOLS = [
    {
        "name": "list_tags",
        "description": "List all tags used in the vault with optional counts",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory to search within",
                },
                "show_counts": {
                    "type": "boolean",
                    "description": "Show usage counts for each tag (default true)",
                },
            },
            "required": [],
        },
    },
    {
        "name": "find_by_tag",
        "description": "Find files containing a specific tag (supports hierarchical tags)",
        "input_schema": {
            "type": "object",
            "properties": {
                "tag": {
                    "type": "string",
                    "description": "Tag to search for (with or without #)",
                },
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory to search within",
                },
            },
            "required": ["tag"],
        },
    },
]


def extract_tags(content: str) -> list[str]:
    """
    Extract tags from content, ignoring those inside code blocks.

    Returns:
        List of unique tags (without # prefix)
    """
    # Remove code blocks to avoid false positives
    clean_content = FENCED_CODE_RE.sub("", content)
    clean_content = INLINE_CODE_RE.sub("", clean_content)

    tags = TAG_PATTERN.findall(clean_content)
    return list(set(tags))


def _list_tags(args: dict) -> dict:
    """List all tags used in the vault with optional counts."""
    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    show_counts = args.get("show_counts", True)

    tag_counter: Counter = Counter()

    for path in search_path.rglob("*.md"):
        if any(p.startswith(".") for p in path.parts):
            continue

        try:
            content = path.read_text(encoding="utf-8")
            tags = extract_tags(content)
            tag_counter.update(tags)
        except Exception:
            pass

    if not tag_counter:
        return {"content": "(no tags found)", "error": None}

    if show_counts:
        # Sort by count descending, then alphabetically
        sorted_tags = sorted(tag_counter.items(), key=lambda x: (-x[1], x[0]))
        lines = [f"#{tag} ({count})" for tag, count in sorted_tags]
    else:
        sorted_tags = sorted(tag_counter.keys())
        lines = [f"#{tag}" for tag in sorted_tags]

    return {"content": "\n".join(lines), "error": None}


def _find_by_tag(args: dict) -> dict:
    """Find files containing a specific tag."""
    tag = args.get("tag", "").lstrip("#")
    if not tag:
        return {"content": "", "error": "Tag required"}

    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    results = []

    for path in search_path.rglob("*.md"):
        if any(p.startswith(".") for p in path.parts):
            continue

        try:
            content = path.read_text(encoding="utf-8")
            tags = extract_tags(content)

            # Check for exact match or hierarchical match
            # e.g., searching for "project" matches both "project" and "project/work"
            for file_tag in tags:
                if file_tag == tag or file_tag.startswith(tag + "/"):
                    rel_path = path.relative_to(get_vault_path())
                    results.append(str(rel_path))
                    break
        except Exception:
            pass

        if len(results) >= 100:
            break

    if not results:
        return {"content": f"No files found with tag #{tag}", "error": None}

    content = "\n".join(sorted(results))
    if len(results) >= 100:
        content += "\n... and more (limited to 100)"

    return {"content": content, "error": None}


HANDLERS = {
    "list_tags": _list_tags,
    "find_by_tag": _find_by_tag,
}
