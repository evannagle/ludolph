"""Tool registry for the MCP server.

Aggregates tools from all domain-specific modules and provides
a unified interface for tool definitions and execution.

Skills in ~/.ludolph/skills/ are auto-discovered and loaded.
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
    conversation,
    directories,
    editing,
    files,
    index,
    memory,
    meta,
    metadata,
    periodic,
    search,
    semantic,
    tags,
    tasks,
    telegram,
    text,
)

# Aggregate core tool definitions
_CORE_TOOLS = (
    files.TOOLS
    + directories.TOOLS
    + search.TOOLS
    + metadata.TOOLS
    + editing.TOOLS
    + index.TOOLS
    + periodic.TOOLS
    + batch.TOOLS
    + tags.TOOLS
    + analysis.TOOLS
    + tasks.TOOLS
    + text.TOOLS
    + backlinks.TOOLS
    + analytics.TOOLS
    + memory.TOOLS
    + meta.TOOLS
    + semantic.TOOLS
    + telegram.TOOLS
    + conversation.TOOLS
)

# Aggregate core handlers
_CORE_HANDLERS = {
    **files.HANDLERS,
    **directories.HANDLERS,
    **search.HANDLERS,
    **metadata.HANDLERS,
    **editing.HANDLERS,
    **index.HANDLERS,
    **periodic.HANDLERS,
    **batch.HANDLERS,
    **tags.HANDLERS,
    **analysis.HANDLERS,
    **tasks.HANDLERS,
    **text.HANDLERS,
    **backlinks.HANDLERS,
    **analytics.HANDLERS,
    **memory.HANDLERS,
    **meta.HANDLERS,
    **semantic.HANDLERS,
    **telegram.HANDLERS,
    **conversation.HANDLERS,
}


def _load_skills() -> tuple[list[dict], dict[str, Any]]:
    """
    Load user-created skills from ~/.ludolph/skills/.

    Each .py file can export:
    - TOOLS: list of tool definitions (dicts)
    - HANDLERS: dict mapping tool names to handler functions

    Returns:
        Tuple of (tools_list, handlers_dict)
    """
    skills_dir = Path.home() / ".ludolph" / "skills"
    legacy_dir = Path.home() / ".ludolph" / "custom_tools"
    tools: list[dict] = []
    handlers: dict[str, Any] = {}

    # Migration: rename custom_tools/ to skills/
    if legacy_dir.exists() and not skills_dir.exists():
        legacy_dir.rename(skills_dir)
        print("Migrated custom_tools/ to skills/")

    if not skills_dir.exists():
        return tools, handlers

    for py_file in sorted(skills_dir.glob("*.py")):
        if py_file.name.startswith("_"):
            continue

        module_name = f"ludolph_skill_{py_file.stem}"
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
            # Log warning but don't crash - skills shouldn't break core
            print(f"Warning: Failed to load skill {py_file.name}: {e}")

    return tools, handlers


# Load skills and combine with core
_skills, _skill_handlers = _load_skills()
TOOLS = list(_CORE_TOOLS) + _skills
HANDLERS = {**_CORE_HANDLERS, **_skill_handlers}


def reload_tools() -> None:
    """Reload skills without restarting the server.

    Called by SIGHUP handler to hot-reload skills.
    """
    global TOOLS, HANDLERS

    _skills, _skill_handlers = _load_skills()
    TOOLS = list(_CORE_TOOLS) + _skills
    HANDLERS = {**_CORE_HANDLERS, **_skill_handlers}

    print(f"Reloaded tools: {len(TOOLS)} total ({len(_skills)} skills)")


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
