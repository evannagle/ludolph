"""Smart editing operations module."""

import re

from ..security import safe_path
from .metadata import FRONTMATTER_RE

TOOLS = [
    {
        "name": "prepend_file",
        "description": "Insert content at the beginning of a file (after frontmatter if present)",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the file relative to root"},
                "content": {"type": "string", "description": "Content to prepend"},
            },
            "required": ["path", "content"],
        },
    },
    {
        "name": "patch_file",
        "description": "Replace content under a specific markdown heading",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the markdown file"},
                "heading": {
                    "type": "string",
                    "description": "Heading text to find (without # prefix)",
                },
                "content": {"type": "string", "description": "New content for the section"},
                "create_if_missing": {
                    "type": "boolean",
                    "description": "Create heading at end if not found (default false)",
                },
            },
            "required": ["path", "heading", "content"],
        },
    },
]


def _prepend_file(args: dict) -> dict:
    """Insert content at the beginning of a file (after frontmatter if present)."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}

    new_content = args.get("content", "")

    # Create parent directories if needed
    path.parent.mkdir(parents=True, exist_ok=True)

    if path.exists():
        existing = path.read_text(encoding="utf-8")

        # Check for frontmatter
        match = FRONTMATTER_RE.match(existing)
        if match:
            # Insert after frontmatter
            frontmatter = existing[: match.end()]
            body = existing[match.end() :]
            combined = frontmatter + new_content + "\n" + body
        else:
            combined = new_content + "\n" + existing

        path.write_text(combined, encoding="utf-8")
    else:
        path.write_text(new_content, encoding="utf-8")

    return {"content": f"Prepended content to {args.get('path')}", "error": None}


def _patch_file(args: dict) -> dict:
    """Replace content under a specific markdown heading."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}
    if not path.is_file():
        return {"content": "", "error": "Path is not a file"}

    heading = args.get("heading", "")
    new_section_content = args.get("content", "")
    create_if_missing = args.get("create_if_missing", False)

    content = path.read_text(encoding="utf-8")

    # Build pattern to find the heading
    # Escape special regex characters in heading text
    escaped_heading = re.escape(heading)
    heading_pattern = re.compile(
        rf"^(#{1,6})\s+{escaped_heading}\s*$", re.IGNORECASE | re.MULTILINE
    )

    match = heading_pattern.search(content)

    if not match:
        if create_if_missing:
            # Append new section at end
            if not content.endswith("\n"):
                content += "\n"
            content += f"\n## {heading}\n\n{new_section_content}\n"
            path.write_text(content, encoding="utf-8")
            return {"content": f"Created new section '{heading}' in {args.get('path')}", "error": None}
        else:
            return {"content": "", "error": f"Heading not found: {heading}"}

    # Found the heading - determine its level
    heading_level = len(match.group(1))

    # Find the end of this section (next heading of same or higher level)
    section_start = match.end()
    next_heading = re.compile(rf"^#{{{1},{heading_level}}}\s+", re.MULTILINE)
    next_match = next_heading.search(content, section_start)

    if next_match:
        section_end = next_match.start()
    else:
        section_end = len(content)

    # Build new content
    before = content[: match.end()]
    after = content[section_end:]

    # Ensure proper spacing
    new_content = before + "\n\n" + new_section_content.strip() + "\n\n" + after.lstrip()

    path.write_text(new_content, encoding="utf-8")
    return {"content": f"Updated section '{heading}' in {args.get('path')}", "error": None}


HANDLERS = {
    "prepend_file": _prepend_file,
    "patch_file": _patch_file,
}
