"""Directory operations module."""

import shutil

from ..security import safe_path

TOOLS = [
    {
        "name": "list_directory",
        "description": "List the contents of a directory",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory relative to root (empty for root)",
                }
            },
            "required": [],
        },
    },
    {
        "name": "create_directory",
        "description": "Create a directory (including parent directories)",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the directory relative to root"}
            },
            "required": ["path"],
        },
    },
    {
        "name": "delete_directory",
        "description": "Delete a directory (requires recursive=true for non-empty directories)",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the directory relative to root"},
                "recursive": {
                    "type": "boolean",
                    "description": "Delete non-empty directories recursively",
                },
            },
            "required": ["path"],
        },
    },
    {
        "name": "file_tree",
        "description": "Display directory structure as ASCII tree",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory relative to root (empty for root)",
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum depth to traverse (default 3)",
                },
                "include_files": {
                    "type": "boolean",
                    "description": "Include files in tree (default true)",
                },
            },
            "required": [],
        },
    },
]


def _list_directory(args: dict) -> dict:
    """List the contents of a directory."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"Directory not found: {args.get('path', '.')}"}
    if not path.is_dir():
        return {"content": "", "error": "Path is not a directory"}

    entries = []
    for entry in sorted(path.iterdir()):
        if entry.name.startswith("."):
            continue
        kind = "dir" if entry.is_dir() else "file"
        entries.append(f"{kind}: {entry.name}")

    content = "\n".join(entries) if entries else "(empty directory)"
    return {"content": content, "error": None}


def _create_directory(args: dict) -> dict:
    """Create a directory (including parent directories)."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}

    path.mkdir(parents=True, exist_ok=True)
    return {"content": f"Created directory: {args.get('path')}", "error": None}


def _delete_directory(args: dict) -> dict:
    """Delete a directory."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"Directory not found: {args.get('path')}"}
    if not path.is_dir():
        return {"content": "", "error": "Path is not a directory"}

    recursive = args.get("recursive", False)

    # Check if directory is empty
    contents = list(path.iterdir())
    if contents and not recursive:
        return {
            "content": "",
            "error": "Directory is not empty. Use recursive=true to delete.",
        }

    if recursive:
        shutil.rmtree(str(path))
    else:
        path.rmdir()

    return {"content": f"Deleted directory: {args.get('path')}", "error": None}


def _file_tree(args: dict) -> dict:
    """Display directory structure as ASCII tree."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"Directory not found: {args.get('path', '.')}"}
    if not path.is_dir():
        return {"content": "", "error": "Path is not a directory"}

    max_depth = args.get("max_depth", 3)
    include_files = args.get("include_files", True)

    lines = [path.name or "."]

    def walk(directory, prefix, depth):
        if depth >= max_depth:
            return

        try:
            entries = sorted(directory.iterdir())
        except PermissionError:
            return

        # Filter hidden files
        entries = [e for e in entries if not e.name.startswith(".")]

        # Filter to directories only if not including files
        if not include_files:
            entries = [e for e in entries if e.is_dir()]

        for i, entry in enumerate(entries):
            is_last = i == len(entries) - 1
            connector = "└── " if is_last else "├── "
            lines.append(f"{prefix}{connector}{entry.name}")

            if entry.is_dir():
                extension = "    " if is_last else "│   "
                walk(entry, prefix + extension, depth + 1)

    walk(path, "", 0)

    return {"content": "\n".join(lines), "error": None}


HANDLERS = {
    "list_directory": _list_directory,
    "create_directory": _create_directory,
    "delete_directory": _delete_directory,
    "file_tree": _file_tree,
}
