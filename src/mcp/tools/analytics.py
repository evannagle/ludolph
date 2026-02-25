"""Vault analytics operations module."""

from datetime import datetime

from ..security import get_vault_path, safe_path
from .analysis import WIKILINK_RE
from .tags import TAG_PATTERN

TOOLS = [
    {
        "name": "vault_stats",
        "description": "Get statistics about the vault (file counts, word counts, orphan notes)",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory to analyze",
                }
            },
            "required": [],
        },
    },
    {
        "name": "date_range",
        "description": "Find files created or modified within a date range",
        "input_schema": {
            "type": "object",
            "properties": {
                "start": {
                    "type": "string",
                    "description": "Start date (YYYY-MM-DD)",
                },
                "end": {
                    "type": "string",
                    "description": "End date (YYYY-MM-DD, default: today)",
                },
                "date_type": {
                    "type": "string",
                    "enum": ["created", "modified"],
                    "description": "Filter by creation or modification date (default: modified)",
                },
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory to search within",
                },
            },
            "required": ["start"],
        },
    },
]


def _vault_stats(args: dict) -> dict:
    """Get statistics about the vault."""
    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    total_files = 0
    total_dirs = 0
    markdown_files = 0
    total_words = 0
    total_links = 0
    total_tags = 0

    all_files = set()
    linked_files = set()

    for path in search_path.rglob("*"):
        if any(p.startswith(".") for p in path.parts):
            continue

        if path.is_dir():
            total_dirs += 1
        elif path.is_file():
            total_files += 1
            rel_path = path.relative_to(get_vault_path())
            all_files.add(str(rel_path))

            if path.suffix == ".md":
                markdown_files += 1

                try:
                    content = path.read_text(encoding="utf-8")

                    # Count words
                    words = len(content.split())
                    total_words += words

                    # Count wikilinks
                    links = WIKILINK_RE.findall(content)
                    total_links += len(links)

                    # Track linked files for orphan detection
                    for link in links:
                        # Check both with and without .md extension
                        linked_files.add(f"{link}.md")
                        linked_files.add(link)

                    # Count tags
                    tags = TAG_PATTERN.findall(content)
                    total_tags += len(tags)

                except Exception:
                    pass

    # Find orphan notes (markdown files with no incoming links)
    orphans = []
    for file_path in all_files:
        if file_path.endswith(".md"):
            name = file_path.split("/")[-1]
            stem = name[:-3] if name.endswith(".md") else name

            # Check if any file links to this one
            if name not in linked_files and stem not in linked_files:
                orphans.append(file_path)

    lines = [
        f"Files: {total_files:,}",
        f"Directories: {total_dirs:,}",
        f"Markdown files: {markdown_files:,}",
        f"Total words: {total_words:,}",
        f"Total links: {total_links:,}",
        f"Total tags: {total_tags:,}",
        f"Orphan notes: {len(orphans):,}",
    ]

    if orphans and len(orphans) <= 20:
        lines.append("\nOrphan notes (no incoming links):")
        for orphan in sorted(orphans)[:20]:
            lines.append(f"  {orphan}")

    return {"content": "\n".join(lines), "error": None}


def _date_range(args: dict) -> dict:
    """Find files created or modified within a date range."""
    start_str = args.get("start", "")
    end_str = args.get("end", "")
    date_type = args.get("date_type", "modified")

    if not start_str:
        return {"content": "", "error": "Start date required"}

    # Parse dates
    try:
        start_date = datetime.strptime(start_str, "%Y-%m-%d")
    except ValueError:
        return {"content": "", "error": "Invalid start date format (use YYYY-MM-DD)"}

    if end_str:
        try:
            end_date = datetime.strptime(end_str, "%Y-%m-%d")
        except ValueError:
            return {"content": "", "error": "Invalid end date format (use YYYY-MM-DD)"}
    else:
        end_date = datetime.now()

    # Set end to end of day
    end_date = end_date.replace(hour=23, minute=59, second=59)

    start_ts = start_date.timestamp()
    end_ts = end_date.timestamp()

    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    results = []

    for path in search_path.rglob("*"):
        if not path.is_file():
            continue
        if any(p.startswith(".") for p in path.parts):
            continue

        try:
            stat = path.stat()

            if date_type == "created":
                file_ts = stat.st_ctime
            else:
                file_ts = stat.st_mtime

            if start_ts <= file_ts <= end_ts:
                rel_path = path.relative_to(get_vault_path())
                file_date = datetime.fromtimestamp(file_ts).strftime("%Y-%m-%d")
                results.append((file_ts, f"{file_date} {rel_path}"))

        except Exception:
            pass

    # Sort by date, newest first
    results.sort(reverse=True)

    if not results:
        return {"content": "(no files found in date range)", "error": None}

    lines = [item[1] for item in results[:50]]
    if len(results) > 50:
        lines.append(f"... and {len(results) - 50} more")

    return {"content": "\n".join(lines), "error": None}


HANDLERS = {
    "vault_stats": _vault_stats,
    "date_range": _date_range,
}
