"""Tool registry for the MCP server.

Aggregates tools from all domain-specific modules and provides
a unified interface for tool definitions and execution.

Custom tools in ~/.ludolph/custom_tools/ are auto-discovered and loaded.
"""

import importlib.util
import sys
from pathlib import Path
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

# Aggregate core tool definitions
_CORE_TOOLS = (
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

# Aggregate core handlers
_CORE_HANDLERS = {
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


def _load_custom_tools() -> tuple[list[dict], dict[str, Any]]:
    """
    Load user/AI-created tools from ~/.ludolph/custom_tools/.

    Each .py file can export:
    - TOOLS: list of tool definitions (dicts)
    - HANDLERS: dict mapping tool names to handler functions

    Returns:
        Tuple of (tools_list, handlers_dict)
    """
    custom_dir = Path.home() / ".ludolph" / "custom_tools"
    tools: list[dict] = []
    handlers: dict[str, Any] = {}

    if not custom_dir.exists():
        return tools, handlers

    for py_file in sorted(custom_dir.glob("*.py")):
        if py_file.name.startswith("_"):
            continue

        module_name = f"ludolph_custom_{py_file.stem}"
        try:
            spec = importlib.util.spec_from_file_location(module_name, py_file)
            if spec is None or spec.loader is None:
                continue

            module = importlib.util.module_from_spec(spec)
            sys.modules[module_name] = module
            spec.loader.exec_module(module)

            if hasattr(module, "TOOLS"):
                tools.extend(module.TOOLS)
            if hasattr(module, "HANDLERS"):
                handlers.update(module.HANDLERS)

        except Exception as e:
            # Log warning but don't crash - custom tools shouldn't break core
            print(f"Warning: Failed to load custom tool {py_file.name}: {e}")

    return tools, handlers


# Load custom tools and combine with core
_custom_tools, _custom_handlers = _load_custom_tools()
TOOLS = list(_CORE_TOOLS) + _custom_tools
HANDLERS = {**_CORE_HANDLERS, **_custom_handlers}


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
