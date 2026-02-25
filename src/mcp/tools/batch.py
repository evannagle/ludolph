"""Batch operations module."""

import json

from ..security import safe_path

TOOLS = [
    {
        "name": "read_files",
        "description": "Read multiple files in a single request (max 20 files)",
        "input_schema": {
            "type": "object",
            "properties": {
                "paths": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "List of file paths relative to root",
                }
            },
            "required": ["paths"],
        },
    },
]


def _read_files(args: dict) -> dict:
    """Read multiple files in a single request."""
    paths = args.get("paths", [])

    if not paths:
        return {"content": "", "error": "No paths provided"}

    # Limit to 20 files to prevent resource exhaustion
    paths = paths[:20]

    results = {}

    for rel_path in paths:
        path = safe_path(rel_path)

        if not path:
            results[rel_path] = {"error": "Invalid path"}
        elif not path.exists():
            results[rel_path] = {"error": "File not found"}
        elif not path.is_file():
            results[rel_path] = {"error": "Not a file"}
        else:
            try:
                content = path.read_text(encoding="utf-8")
                results[rel_path] = {"content": content}
            except Exception as e:
                results[rel_path] = {"error": str(e)}

    return {"content": json.dumps(results, indent=2), "error": None}


HANDLERS = {
    "read_files": _read_files,
}
