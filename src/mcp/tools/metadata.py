"""Metadata and frontmatter operations module."""

import json
import re
from datetime import datetime

from ..security import is_git_ignored, safe_path

# Regex to match YAML frontmatter block
FRONTMATTER_RE = re.compile(r"^---\s*\n(.*?)\n---\s*\n?", re.DOTALL)

TOOLS = [
    {
        "name": "file_info",
        "description": "Get file metadata (size, dates, permissions)",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to root"}
            },
            "required": ["path"],
        },
    },
    {
        "name": "get_frontmatter",
        "description": "Extract YAML frontmatter from a markdown file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the markdown file"}
            },
            "required": ["path"],
        },
    },
    {
        "name": "update_frontmatter",
        "description": "Update or add YAML frontmatter fields (set value to null to delete a field)",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the markdown file"},
                "updates": {
                    "type": "object",
                    "description": "Key-value pairs to update (null to delete)",
                },
            },
            "required": ["path", "updates"],
        },
    },
]


def parse_frontmatter(content: str) -> tuple[dict, str]:
    """
    Parse YAML frontmatter from content.

    Returns:
        Tuple of (frontmatter dict, body content)
    """
    match = FRONTMATTER_RE.match(content)
    if not match:
        return {}, content

    yaml_content = match.group(1)
    body = content[match.end() :]

    # Simple YAML parser for common frontmatter patterns
    frontmatter = {}
    current_key = None
    current_value = []

    for line in yaml_content.split("\n"):
        # Skip empty lines
        if not line.strip():
            continue

        # Check for key: value
        if ":" in line and not line.startswith(" ") and not line.startswith("\t"):
            # Save previous key if exists
            if current_key is not None:
                frontmatter[current_key] = _parse_yaml_value("\n".join(current_value))

            parts = line.split(":", 1)
            current_key = parts[0].strip()
            value = parts[1].strip() if len(parts) > 1 else ""

            if value:
                current_value = [value]
            else:
                current_value = []
        elif current_key is not None:
            # Continuation of previous value (indented)
            current_value.append(line)

    # Save last key
    if current_key is not None:
        frontmatter[current_key] = _parse_yaml_value("\n".join(current_value))

    return frontmatter, body


def _parse_yaml_value(value: str):
    """Parse a YAML value string into Python type."""
    value = value.strip()

    if not value:
        return None

    # Boolean
    if value.lower() in ("true", "yes"):
        return True
    if value.lower() in ("false", "no"):
        return False

    # Integer
    if value.isdigit() or (value.startswith("-") and value[1:].isdigit()):
        return int(value)

    # List (bracket notation)
    if value.startswith("[") and value.endswith("]"):
        items = value[1:-1].split(",")
        return [item.strip().strip("\"'") for item in items if item.strip()]

    # List (dash notation)
    if value.startswith("-"):
        items = []
        for line in value.split("\n"):
            line = line.strip()
            if line.startswith("-"):
                items.append(line[1:].strip().strip("\"'"))
        return items

    # Quoted string
    if (value.startswith('"') and value.endswith('"')) or (
        value.startswith("'") and value.endswith("'")
    ):
        return value[1:-1]

    return value


def serialize_frontmatter(frontmatter: dict) -> str:
    """Serialize frontmatter dict back to YAML string."""
    lines = []
    for key, value in frontmatter.items():
        if value is None:
            continue
        elif isinstance(value, bool):
            lines.append(f"{key}: {str(value).lower()}")
        elif isinstance(value, int):
            lines.append(f"{key}: {value}")
        elif isinstance(value, list):
            if all(isinstance(item, str) and len(item) < 30 for item in value):
                # Inline format for simple lists
                items = ", ".join(value)
                lines.append(f"{key}: [{items}]")
            else:
                # Multiline format
                lines.append(f"{key}:")
                for item in value:
                    lines.append(f"  - {item}")
        elif isinstance(value, str):
            # Quote if contains special characters
            if any(c in value for c in ':{}[]#&*!|>\'"%@`'):
                lines.append(f'{key}: "{value}"')
            else:
                lines.append(f"{key}: {value}")
        else:
            lines.append(f"{key}: {value}")

    return "\n".join(lines)


def _file_info(args: dict) -> dict:
    """Get file metadata."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}

    stat = path.stat()

    info_lines = [
        f"path: {args.get('path')}",
        f"type: {'directory' if path.is_dir() else 'file'}",
        f"size: {stat.st_size} bytes",
        f"created: {datetime.fromtimestamp(stat.st_ctime).isoformat()}",
        f"modified: {datetime.fromtimestamp(stat.st_mtime).isoformat()}",
        f"permissions: {oct(stat.st_mode)[-3:]}",
    ]

    # Add git status if in a git repo
    if not path.is_dir() and is_git_ignored(path):
        info_lines.append("git: ignored")

    return {"content": "\n".join(info_lines), "error": None}


def _get_frontmatter(args: dict) -> dict:
    """Extract YAML frontmatter from a markdown file."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}
    if not path.is_file():
        return {"content": "", "error": "Path is not a file"}

    content = path.read_text(encoding="utf-8")
    frontmatter, _ = parse_frontmatter(content)

    if not frontmatter:
        return {"content": "(no frontmatter)", "error": None}

    return {"content": json.dumps(frontmatter, indent=2), "error": None}


def _update_frontmatter(args: dict) -> dict:
    """Update or add YAML frontmatter fields."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}
    if not path.is_file():
        return {"content": "", "error": "Path is not a file"}

    updates = args.get("updates", {})
    if not updates:
        return {"content": "", "error": "No updates provided"}

    content = path.read_text(encoding="utf-8")
    frontmatter, body = parse_frontmatter(content)

    # Apply updates
    for key, value in updates.items():
        if value is None:
            frontmatter.pop(key, None)  # Delete field
        else:
            frontmatter[key] = value

    # Rebuild content
    yaml_content = serialize_frontmatter(frontmatter)
    new_content = f"---\n{yaml_content}\n---\n{body}"

    path.write_text(new_content, encoding="utf-8")

    return {"content": f"Updated frontmatter in {args.get('path')}", "error": None}


HANDLERS = {
    "file_info": _file_info,
    "get_frontmatter": _get_frontmatter,
    "update_frontmatter": _update_frontmatter,
}
