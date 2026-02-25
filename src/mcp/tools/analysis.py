"""Document analysis operations module."""

import re

from ..security import safe_path

# Regex for wikilinks: [[target]] or [[target|alias]]
WIKILINK_RE = re.compile(r"\[\[([^\]|]+)(?:\|[^\]]+)?\]\]")

# Regex for external URLs (markdown links or bare URLs)
URL_RE = re.compile(r"\[([^\]]+)\]\((https?://[^\)]+)\)|(?<!\()https?://[^\s\)]+")

# Regex for embeds: ![[file]] or ![[file|display]]
EMBED_RE = re.compile(r"!\[\[([^\]|]+)(?:\|[^\]]+)?\]\]")

# Regex for footnote definitions and references
FOOTNOTE_DEF_RE = re.compile(r"^\[\^([^\]]+)\]:\s*(.+)$", re.MULTILINE)
FOOTNOTE_REF_RE = re.compile(r"\[\^([^\]]+)\]")

TOOLS = [
    {
        "name": "document_outline",
        "description": "Extract heading structure from a markdown file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the markdown file"},
                "include_line_numbers": {
                    "type": "boolean",
                    "description": "Include line numbers (default true)",
                },
            },
            "required": ["path"],
        },
    },
    {
        "name": "get_links",
        "description": "Extract all links from a markdown file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the markdown file"},
                "link_type": {
                    "type": "string",
                    "enum": ["all", "wikilinks", "external"],
                    "description": "Type of links to extract (default: all)",
                },
            },
            "required": ["path"],
        },
    },
    {
        "name": "get_footnotes",
        "description": "Extract footnote definitions and references from a markdown file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the markdown file"}
            },
            "required": ["path"],
        },
    },
    {
        "name": "get_embeds",
        "description": "Extract embedded content references (![[file]]) from a markdown file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to the markdown file"}
            },
            "required": ["path"],
        },
    },
]


def _document_outline(args: dict) -> dict:
    """Extract heading structure from a markdown file."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}
    if not path.is_file():
        return {"content": "", "error": "Path is not a file"}

    include_line_numbers = args.get("include_line_numbers", True)

    content = path.read_text(encoding="utf-8")
    lines = content.split("\n")

    outline = []
    heading_pattern = re.compile(r"^(#{1,6})\s+(.+)$")

    for i, line in enumerate(lines, 1):
        match = heading_pattern.match(line)
        if match:
            level = len(match.group(1))
            text = match.group(2).strip()
            indent = "  " * (level - 1)

            if include_line_numbers:
                outline.append(f"{indent}{text} (line {i})")
            else:
                outline.append(f"{indent}{text}")

    if not outline:
        return {"content": "(no headings found)", "error": None}

    return {"content": "\n".join(outline), "error": None}


def _get_links(args: dict) -> dict:
    """Extract all links from a markdown file."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}
    if not path.is_file():
        return {"content": "", "error": "Path is not a file"}

    link_type = args.get("link_type", "all")

    content = path.read_text(encoding="utf-8")

    wikilinks = set()
    external_urls = set()

    # Extract wikilinks
    if link_type in ("all", "wikilinks"):
        for match in WIKILINK_RE.finditer(content):
            wikilinks.add(match.group(1))

    # Extract external URLs
    if link_type in ("all", "external"):
        for match in URL_RE.finditer(content):
            if match.group(2):  # Markdown link [text](url)
                external_urls.add(match.group(2))
            else:  # Bare URL
                external_urls.add(match.group(0))

    lines = []
    if wikilinks:
        lines.append("Wikilinks:")
        for link in sorted(wikilinks):
            lines.append(f"  [[{link}]]")

    if external_urls:
        if lines:
            lines.append("")
        lines.append("External URLs:")
        for url in sorted(external_urls):
            lines.append(f"  {url}")

    if not lines:
        return {"content": "(no links found)", "error": None}

    return {"content": "\n".join(lines), "error": None}


def _get_footnotes(args: dict) -> dict:
    """Extract footnote definitions and references from a markdown file."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}
    if not path.is_file():
        return {"content": "", "error": "Path is not a file"}

    content = path.read_text(encoding="utf-8")

    # Extract definitions
    definitions = {}
    for match in FOOTNOTE_DEF_RE.finditer(content):
        definitions[match.group(1)] = match.group(2)

    # Extract references
    references = set()
    for match in FOOTNOTE_REF_RE.finditer(content):
        references.add(match.group(1))

    # Find undefined references
    undefined = references - set(definitions.keys())

    lines = []
    if definitions:
        lines.append("Definitions:")
        for name, text in sorted(definitions.items()):
            lines.append(f"  [^{name}]: {text}")

    if undefined:
        if lines:
            lines.append("")
        lines.append("Undefined references:")
        for name in sorted(undefined):
            lines.append(f"  [^{name}]")

    if not lines:
        return {"content": "(no footnotes found)", "error": None}

    return {"content": "\n".join(lines), "error": None}


def _get_embeds(args: dict) -> dict:
    """Extract embedded content references from a markdown file."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}
    if not path.exists():
        return {"content": "", "error": f"File not found: {args.get('path')}"}
    if not path.is_file():
        return {"content": "", "error": "Path is not a file"}

    content = path.read_text(encoding="utf-8")

    embeds = set()
    for match in EMBED_RE.finditer(content):
        embeds.add(match.group(1))

    if not embeds:
        return {"content": "(no embeds found)", "error": None}

    lines = ["Embedded files:"]
    for embed in sorted(embeds):
        lines.append(f"  ![[{embed}]]")

    return {"content": "\n".join(lines), "error": None}


HANDLERS = {
    "document_outline": _document_outline,
    "get_links": _get_links,
    "get_footnotes": _get_footnotes,
    "get_embeds": _get_embeds,
}
