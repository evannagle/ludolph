"""Tool registry for the MCP server.

Aggregates tools from all domain-specific modules and provides
a unified interface for tool definitions and execution.
"""

from typing import Any

from . import (
    analysis,
    analytics,
    backlinks,
    batch,
    directories,
    editing,
    files,
    metadata,
    periodic,
    search,
    tags,
    tasks,
    text,
)

# Aggregate all tool definitions
TOOLS = (
    files.TOOLS
    + directories.TOOLS
    + search.TOOLS
    + metadata.TOOLS
    + editing.TOOLS
    + periodic.TOOLS
    + batch.TOOLS
    + tags.TOOLS
    + analysis.TOOLS
    + tasks.TOOLS
    + text.TOOLS
    + backlinks.TOOLS
    + analytics.TOOLS
)

# Aggregate all handlers
HANDLERS = {
    **files.HANDLERS,
    **directories.HANDLERS,
    **search.HANDLERS,
    **metadata.HANDLERS,
    **editing.HANDLERS,
    **periodic.HANDLERS,
    **batch.HANDLERS,
    **tags.HANDLERS,
    **analysis.HANDLERS,
    **tasks.HANDLERS,
    **text.HANDLERS,
    **backlinks.HANDLERS,
    **analytics.HANDLERS,
}


def get_tool_definitions() -> list[dict]:
    """Return all tool definitions."""
    return TOOLS


def call_tool(name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    """
    Execute a tool and return the result.

    Returns:
        Dict with 'content' (str) and 'error' (str|None) keys
    """
    handler = HANDLERS.get(name)
    if not handler:
        return {"content": "", "error": f"Unknown tool: {name}"}

    try:
        return handler(arguments)
    except Exception as e:
        return {"content": "", "error": str(e)}
