"""Task extraction operations module."""

import re

from ..security import get_vault_path, safe_path

# Pattern for markdown checkboxes: - [ ] or * [x] or - [X]
TASK_PATTERN = re.compile(r"^\s*[-*]\s*\[([ xX])\]\s*(.+)$", re.MULTILINE)

TOOLS = [
    {
        "name": "extract_tasks",
        "description": "Extract checkbox tasks from markdown files",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to file or directory to search",
                },
                "status": {
                    "type": "string",
                    "enum": ["all", "open", "completed"],
                    "description": "Filter by task status (default: all)",
                },
                "include_context": {
                    "type": "boolean",
                    "description": "Include file path with each task (default true)",
                },
            },
            "required": [],
        },
    },
]


def _extract_tasks(args: dict) -> dict:
    """Extract checkbox tasks from markdown files."""
    path_arg = args.get("path", "")
    status_filter = args.get("status", "all")
    include_context = args.get("include_context", True)

    search_path = safe_path(path_arg) if path_arg else get_vault_path()
    if not search_path:
        return {"content": "", "error": "Invalid path"}

    tasks = []

    # Determine files to search
    if search_path.is_file():
        files = [search_path]
    else:
        files = list(search_path.rglob("*.md"))

    for file_path in files:
        if any(p.startswith(".") for p in file_path.parts):
            continue

        try:
            content = file_path.read_text(encoding="utf-8")
            rel_path = file_path.relative_to(get_vault_path())

            for match in TASK_PATTERN.finditer(content):
                checkbox = match.group(1)
                task_text = match.group(2).strip()

                # Determine completion status
                is_completed = checkbox.lower() == "x"

                # Apply filter
                if status_filter == "open" and is_completed:
                    continue
                if status_filter == "completed" and not is_completed:
                    continue

                # Format task
                marker = "[x]" if is_completed else "[ ]"
                if include_context:
                    tasks.append(f"- {marker} {task_text} ({rel_path})")
                else:
                    tasks.append(f"- {marker} {task_text}")

        except Exception:
            pass

    if not tasks:
        return {"content": "(no tasks found)", "error": None}

    return {"content": "\n".join(tasks), "error": None}


HANDLERS = {
    "extract_tasks": _extract_tasks,
}
