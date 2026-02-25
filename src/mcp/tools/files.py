"""Core file operations module."""

import shutil

from ..security import safe_path

TOOLS = [
    {
        "name": "read_file",
        "description": "Read the contents of a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to root"}
            },
            "required": ["path"],
        },
    },
    {
        "name": "write_file",
        "description": "Create or replace a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to root"},
                "content": {"type": "string", "description": "Content to write to the file"},
            },
            "required": ["path", "content"],
        },
    },
    {
        "name": "append_file",
        "description": "Append content to the end of a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to root"},
                "content": {"type": "string", "description": "Content to append"},
            },
            "required": ["path", "content"],
        },
    },
    {
        "name": "delete_file",
        "description": "Delete a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to root"}
            },
            "required": ["path"],
        },
    },
    {
        "name": "move_file",
        "description": "Move or rename a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "source": {"type": "string", "description": "Source path relative to root"},
                "destination": {
                    "type": "string",
                    "description": "Destination path relative to root",
                },
            },
            "required": ["source", "destination"],
        },
    },
    {
        "name": "copy_file",
        "description": "Copy a file to a new location",
        "input_schema": {
            "type": "object",
            "properties": {
                "source": {"type": "string", "description": "Source path relative to root"},
                "destination": {
                    "type": "string",
                    "description": "Destination path relative to root",
                },
            },
            "required": ["source", "destination"],
        },
    },
]


def _read_file(args: dict) -> dict:
    """Read the contents of a file."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}
    if not path.is_file():
        return {"content": "", "error": "Path is not a file"}

    content = path.read_text(encoding="utf-8")
    return {"content": content, "error": None}


def _write_file(args: dict) -> dict:
    """Create or replace a file."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}

    content = args.get("content", "")

    # Create parent directories if needed
    path.parent.mkdir(parents=True, exist_ok=True)

    path.write_text(content, encoding="utf-8")
    return {"content": f"Written {len(content)} bytes to {args.get('path')}", "error": None}


def _append_file(args: dict) -> dict:
    """Append content to the end of a file."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}

    content = args.get("content", "")

    # Create parent directories if needed
    path.parent.mkdir(parents=True, exist_ok=True)

    # Append with smart newline handling
    if path.exists():
        existing = path.read_text(encoding="utf-8")
        if existing and not existing.endswith("\n"):
            content = "\n" + content
        with path.open("a", encoding="utf-8") as f:
            f.write(content)
    else:
        path.write_text(content, encoding="utf-8")

    return {"content": f"Appended {len(content)} bytes to {args.get('path')}", "error": None}


def _delete_file(args: dict) -> dict:
    """Delete a file."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}
    if not path.is_file():
        return {"content": "", "error": "Path is not a file"}

    path.unlink()
    return {"content": f"Deleted {args.get('path')}", "error": None}


def _move_file(args: dict) -> dict:
    """Move or rename a file."""
    source = safe_path(args.get("source", ""))
    destination = safe_path(args.get("destination", ""))

    if not source:
        return {"content": "", "error": "Invalid source path"}
    if not destination:
        return {"content": "", "error": "Invalid destination path"}
    if not source.exists():
        return {"content": "", "error": f"Source not found: {args.get('source')}"}

    # Create parent directories if needed
    destination.parent.mkdir(parents=True, exist_ok=True)

    shutil.move(str(source), str(destination))
    return {"content": f"Moved {args.get('source')} to {args.get('destination')}", "error": None}


def _copy_file(args: dict) -> dict:
    """Copy a file to a new location, preserving metadata."""
    source = safe_path(args.get("source", ""))
    destination = safe_path(args.get("destination", ""))

    if not source:
        return {"content": "", "error": "Invalid source path"}
    if not destination:
        return {"content": "", "error": "Invalid destination path"}
    if not source.exists():
        return {"content": "", "error": f"Source not found: {args.get('source')}"}
    if not source.is_file():
        return {"content": "", "error": "Source is not a file"}

    # Create parent directories if needed
    destination.parent.mkdir(parents=True, exist_ok=True)

    shutil.copy2(str(source), str(destination))
    return {"content": f"Copied {args.get('source')} to {args.get('destination')}", "error": None}


HANDLERS = {
    "read_file": _read_file,
    "write_file": _write_file,
    "append_file": _append_file,
    "delete_file": _delete_file,
    "move_file": _move_file,
    "copy_file": _copy_file,
}
