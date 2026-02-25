"""Text operations module."""

import re

from ..security import get_vault_path, safe_path

# Pattern for fenced code blocks: ```language\ncode\n```
CODE_BLOCK_RE = re.compile(r"```(\w*)\n(.*?)```", re.DOTALL)

TOOLS = [
    {
        "name": "find_replace",
        "description": "Find and replace text across files (dry run by default)",
        "input_schema": {
            "type": "object",
            "properties": {
                "find": {"type": "string", "description": "Text or regex pattern to find"},
                "replace": {"type": "string", "description": "Replacement text"},
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory to search within",
                },
                "regex": {
                    "type": "boolean",
                    "description": "Treat find as regex pattern (default false)",
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview changes without applying (default true)",
                },
            },
            "required": ["find", "replace"],
        },
    },
    {
        "name": "extract_code_blocks",
        "description": "Extract fenced code blocks from markdown files",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to file or directory"},
                "language": {
                    "type": "string",
                    "description": "Filter by language (e.g., 'python', 'javascript')",
                },
            },
            "required": ["path"],
        },
    },
    {
        "name": "extract_quotes",
        "description": "Extract blockquotes from markdown files",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to file or directory"}
            },
            "required": ["path"],
        },
    },
]


# File extensions allowed for find/replace
ALLOWED_EXTENSIONS = {".md", ".txt", ".json", ".yaml", ".yml"}


def _find_replace(args: dict) -> dict:
    """Find and replace text across files."""
    find_text = args.get("find", "")
    replace_text = args.get("replace", "")
    use_regex = args.get("regex", False)
    dry_run = args.get("dry_run", True)

    if not find_text:
        return {"content": "", "error": "Find text required"}

    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    # Build pattern
    try:
        if use_regex:
            pattern = re.compile(find_text, re.MULTILINE)
        else:
            pattern = re.compile(re.escape(find_text), re.MULTILINE)
    except re.error as e:
        return {"content": "", "error": f"Invalid regex: {e}"}

    results = []
    total_matches = 0

    for file_path in search_path.rglob("*"):
        if file_path.suffix not in ALLOWED_EXTENSIONS:
            continue
        if any(p.startswith(".") for p in file_path.parts):
            continue

        try:
            content = file_path.read_text(encoding="utf-8")
            matches = list(pattern.finditer(content))

            if matches:
                rel_path = file_path.relative_to(get_vault_path())
                match_count = len(matches)
                total_matches += match_count
                results.append(f"{rel_path}: {match_count} match(es)")

                if not dry_run:
                    new_content = pattern.sub(replace_text, content)
                    file_path.write_text(new_content, encoding="utf-8")

        except Exception:
            pass

    if not results:
        return {"content": "No matches found", "error": None}

    header = f"{'[DRY RUN] ' if dry_run else ''}Found {total_matches} match(es) in {len(results)} file(s):\n"
    content = header + "\n".join(results)

    if not dry_run:
        content += f"\n\nReplaced all occurrences."

    return {"content": content, "error": None}


def _extract_code_blocks(args: dict) -> dict:
    """Extract fenced code blocks from markdown files."""
    path_arg = args.get("path", "")
    language_filter = args.get("language", "").lower()

    search_path = safe_path(path_arg)
    if not search_path:
        return {"content": "", "error": "Invalid path"}

    # Determine files to search
    if search_path.is_file():
        files = [search_path]
    else:
        files = list(search_path.rglob("*.md"))

    blocks = []

    for file_path in files:
        if any(p.startswith(".") for p in file_path.parts):
            continue

        try:
            content = file_path.read_text(encoding="utf-8")
            rel_path = file_path.relative_to(get_vault_path())

            for match in CODE_BLOCK_RE.finditer(content):
                lang = match.group(1) or "text"
                code = match.group(2).strip()

                # Apply language filter
                if language_filter and lang.lower() != language_filter:
                    continue

                blocks.append(f"--- {rel_path} ({lang}) ---\n{code}")

        except Exception:
            pass

    if not blocks:
        return {"content": "(no code blocks found)", "error": None}

    return {"content": "\n\n".join(blocks), "error": None}


def _extract_quotes(args: dict) -> dict:
    """Extract blockquotes from markdown files."""
    path_arg = args.get("path", "")

    search_path = safe_path(path_arg)
    if not search_path:
        return {"content": "", "error": "Invalid path"}

    # Determine files to search
    if search_path.is_file():
        files = [search_path]
    else:
        files = list(search_path.rglob("*.md"))

    quotes = []

    for file_path in files:
        if any(p.startswith(".") for p in file_path.parts):
            continue

        try:
            content = file_path.read_text(encoding="utf-8")
            rel_path = file_path.relative_to(get_vault_path())
            lines = content.split("\n")

            current_quote = []
            in_quote = False

            for line in lines:
                if line.startswith(">"):
                    in_quote = True
                    # Remove > prefix and optional space
                    quote_line = line[1:].lstrip() if len(line) > 1 else ""
                    current_quote.append(quote_line)
                else:
                    if in_quote and current_quote:
                        quote_text = "\n".join(current_quote)
                        quotes.append(f"--- {rel_path} ---\n{quote_text}")
                        current_quote = []
                    in_quote = False

            # Don't forget trailing quote
            if current_quote:
                quote_text = "\n".join(current_quote)
                quotes.append(f"--- {rel_path} ---\n{quote_text}")

        except Exception:
            pass

    if not quotes:
        return {"content": "(no blockquotes found)", "error": None}

    return {"content": "\n\n".join(quotes), "error": None}


HANDLERS = {
    "find_replace": _find_replace,
    "extract_code_blocks": _extract_code_blocks,
    "extract_quotes": _extract_quotes,
}
