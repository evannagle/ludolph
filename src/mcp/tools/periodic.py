"""Periodic notes operations module."""

from datetime import datetime

from ..security import safe_path

TOOLS = [
    {
        "name": "periodic_note",
        "description": "Get or create a periodic note (daily, weekly, monthly, yearly)",
        "input_schema": {
            "type": "object",
            "properties": {
                "period": {
                    "type": "string",
                    "enum": ["daily", "weekly", "monthly", "yearly"],
                    "description": "Type of periodic note",
                },
                "date": {
                    "type": "string",
                    "description": "Date in YYYY-MM-DD format (default: today)",
                },
                "folder": {
                    "type": "string",
                    "description": "Folder for periodic notes (default: journal)",
                },
            },
            "required": ["period"],
        },
    },
    {
        "name": "append_periodic",
        "description": "Append content to a periodic note, creating if needed",
        "input_schema": {
            "type": "object",
            "properties": {
                "period": {
                    "type": "string",
                    "enum": ["daily", "weekly", "monthly", "yearly"],
                    "description": "Type of periodic note",
                },
                "content": {"type": "string", "description": "Content to append"},
                "date": {
                    "type": "string",
                    "description": "Date in YYYY-MM-DD format (default: today)",
                },
                "folder": {
                    "type": "string",
                    "description": "Folder for periodic notes (default: journal)",
                },
            },
            "required": ["period", "content"],
        },
    },
]


def _get_periodic_path(period: str, date_str: str | None, folder: str) -> tuple[str, str]:
    """
    Get the path for a periodic note.

    Returns:
        Tuple of (relative_path, filename_without_extension)
    """
    # Parse date or use today
    if date_str:
        try:
            date = datetime.strptime(date_str, "%Y-%m-%d").date()
        except ValueError:
            date = datetime.now().date()
    else:
        date = datetime.now().date()

    # Generate filename based on period
    if period == "daily":
        filename = date.strftime("%Y-%m-%d")
    elif period == "weekly":
        # ISO week format: YYYY-Wnn
        year, week, _ = date.isocalendar()
        filename = f"{year}-W{week:02d}"
    elif period == "monthly":
        filename = date.strftime("%Y-%m")
    elif period == "yearly":
        filename = date.strftime("%Y")
    else:
        filename = date.strftime("%Y-%m-%d")

    return f"{folder}/{filename}.md", filename


def _periodic_note(args: dict) -> dict:
    """Get or create a periodic note."""
    period = args.get("period", "daily")
    date_str = args.get("date")
    folder = args.get("folder", "journal")

    rel_path, filename = _get_periodic_path(period, date_str, folder)
    path = safe_path(rel_path)

    if not path:
        return {"content": "", "error": "Invalid path"}

    if path.exists():
        content = path.read_text(encoding="utf-8")
        return {"content": content, "error": None}
    else:
        return {"content": f"(Note does not exist: {rel_path})", "error": None}


def _append_periodic(args: dict) -> dict:
    """Append content to a periodic note, creating if needed."""
    period = args.get("period", "daily")
    content = args.get("content", "")
    date_str = args.get("date")
    folder = args.get("folder", "journal")

    if not content:
        return {"content": "", "error": "Content required"}

    rel_path, filename = _get_periodic_path(period, date_str, folder)
    path = safe_path(rel_path)

    if not path:
        return {"content": "", "error": "Invalid path"}

    # Create parent directories if needed
    path.parent.mkdir(parents=True, exist_ok=True)

    if path.exists():
        # Append to existing file with smart newline handling
        existing = path.read_text(encoding="utf-8")
        if existing and not existing.endswith("\n"):
            content = "\n" + content
        with path.open("a", encoding="utf-8") as f:
            f.write(content)
        return {"content": f"Appended to {rel_path}", "error": None}
    else:
        # Create new file with frontmatter
        now = datetime.now().isoformat()
        frontmatter = f"---\ntitle: {filename}\ncreated: {now}\n---\n\n"
        path.write_text(frontmatter + content, encoding="utf-8")
        return {"content": f"Created {rel_path}", "error": None}


HANDLERS = {
    "periodic_note": _periodic_note,
    "append_periodic": _append_periodic,
}
