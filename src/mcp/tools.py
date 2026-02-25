"""Tool definitions and implementations for the MCP server."""

import os
import re
import shutil
from datetime import datetime
from pathlib import Path
from typing import Any

from .security import get_vault_path, safe_path, is_git_ignored

# Tool definitions for /tools endpoint
TOOLS = [
    # Phase 1: Core File Operations
    {
        "name": "read_file",
        "description": "Read the contents of a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file relative to root"
                }
            },
            "required": ["path"]
        }
    },
    {
        "name": "write_file",
        "description": "Create or replace a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file relative to root"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        }
    },
    {
        "name": "append_file",
        "description": "Append content to the end of a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file relative to root"
                },
                "content": {
                    "type": "string",
                    "description": "Content to append"
                }
            },
            "required": ["path", "content"]
        }
    },
    {
        "name": "delete_file",
        "description": "Delete a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file relative to root"
                }
            },
            "required": ["path"]
        }
    },
    {
        "name": "move_file",
        "description": "Move or rename a file",
        "input_schema": {
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Source path relative to root"
                },
                "destination": {
                    "type": "string",
                    "description": "Destination path relative to root"
                }
            },
            "required": ["source", "destination"]
        }
    },
    # Phase 2: Directory Operations
    {
        "name": "list_directory",
        "description": "List the contents of a directory",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory relative to root (empty for root)"
                }
            },
            "required": []
        }
    },
    {
        "name": "create_directory",
        "description": "Create a directory (including parent directories)",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory relative to root"
                }
            },
            "required": ["path"]
        }
    },
    # Phase 3: Search
    {
        "name": "search",
        "description": "Search for files or content (simple text search)",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (searches file names and content)"
                },
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory to search within"
                },
                "context_length": {
                    "type": "integer",
                    "description": "Number of characters of context around matches (default 50)"
                }
            },
            "required": ["query"]
        }
    },
    {
        "name": "search_advanced",
        "description": "Advanced search with regex and glob patterns",
        "input_schema": {
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Optional subdirectory to search within"
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g., '*.md')"
                },
                "content_only": {
                    "type": "boolean",
                    "description": "Search only file content, not names"
                }
            },
            "required": ["pattern"]
        }
    },
    # Phase 4: Metadata
    {
        "name": "file_info",
        "description": "Get file metadata (size, dates, permissions)",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file relative to root"
                }
            },
            "required": ["path"]
        }
    },
]


def get_tool_definitions() -> list[dict]:
    """Return all tool definitions."""
    return TOOLS


def call_tool(name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    """
    Execute a tool and return the result.

    Returns:
        Dict with 'content' (str) and 'error' (str|None) keys
    """
    handlers = {
        "read_file": _read_file,
        "write_file": _write_file,
        "append_file": _append_file,
        "delete_file": _delete_file,
        "move_file": _move_file,
        "list_directory": _list_directory,
        "create_directory": _create_directory,
        "search": _search,
        "search_advanced": _search_advanced,
        "file_info": _file_info,
    }

    handler = handlers.get(name)
    if not handler:
        return {"content": "", "error": f"Unknown tool: {name}"}

    try:
        return handler(arguments)
    except Exception as e:
        return {"content": "", "error": str(e)}


# =============================================================================
# Phase 1: Core File Operations
# =============================================================================

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
    return {
        "content": f"Moved {args.get('source')} to {args.get('destination')}",
        "error": None
    }


# =============================================================================
# Phase 2: Directory Operations
# =============================================================================

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


# =============================================================================
# Phase 3: Search
# =============================================================================

def _search(args: dict) -> dict:
    """Simple text search across file names and content."""
    query = args.get("query", "")
    if not query:
        return {"content": "", "error": "Query required"}

    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    context_length = args.get("context_length", 50)

    results = []
    pattern = re.compile(re.escape(query), re.IGNORECASE)

    for path in search_path.rglob("*"):
        if path.is_file() and not any(p.startswith(".") for p in path.parts):
            rel_path = path.relative_to(get_vault_path())

            # Check filename match
            if pattern.search(path.name):
                results.append(f"file: {rel_path}")

            # Check content for text files
            elif path.suffix in (".md", ".txt", ".json", ".yaml", ".yml", ".py", ".js", ".ts", ".rs"):
                try:
                    content = path.read_text(encoding="utf-8")
                    match = pattern.search(content)
                    if match:
                        # Extract context around match
                        start = max(0, match.start() - context_length)
                        end = min(len(content), match.end() + context_length)
                        context = content[start:end].replace("\n", " ")
                        results.append(f"match: {rel_path}\n  ...{context}...")
                except Exception:
                    pass

        if len(results) >= 50:
            break

    content = "\n".join(results) if results else "No matches found"
    return {"content": content, "error": None}


def _search_advanced(args: dict) -> dict:
    """Advanced search with regex and glob patterns."""
    pattern_str = args.get("pattern", "")
    if not pattern_str:
        return {"content": "", "error": "Pattern required"}

    try:
        pattern = re.compile(pattern_str, re.IGNORECASE | re.MULTILINE)
    except re.error as e:
        return {"content": "", "error": f"Invalid regex: {e}"}

    search_path = safe_path(args.get("path", ""))
    if not search_path:
        search_path = get_vault_path()

    glob_pattern = args.get("glob", "*")
    content_only = args.get("content_only", False)

    results = []

    for path in search_path.rglob(glob_pattern):
        if path.is_file() and not any(p.startswith(".") for p in path.parts):
            rel_path = path.relative_to(get_vault_path())

            # Check filename match (unless content_only)
            if not content_only and pattern.search(path.name):
                results.append(f"file: {rel_path}")

            # Check content
            try:
                content = path.read_text(encoding="utf-8")
                matches = list(pattern.finditer(content))
                if matches:
                    for match in matches[:3]:  # Limit matches per file
                        start = max(0, match.start() - 30)
                        end = min(len(content), match.end() + 30)
                        context = content[start:end].replace("\n", " ")
                        results.append(f"match: {rel_path}:{match.start()}\n  ...{context}...")
            except Exception:
                pass

        if len(results) >= 50:
            break

    content = "\n".join(results) if results else "No matches found"
    return {"content": content, "error": None}


# =============================================================================
# Phase 4: Metadata
# =============================================================================

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
