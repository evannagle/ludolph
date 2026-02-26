"""Meta-tools for dynamic tool creation and management.

Allows Lu to create, list, and delete custom tools at runtime.
Custom tools are stored in ~/.ludolph/custom_tools/ and auto-loaded.
"""

import os
import re
import signal
from pathlib import Path

# Custom tools directory
CUSTOM_TOOLS_DIR = Path.home() / ".ludolph" / "custom_tools"

# Forbidden patterns in tool code (security)
FORBIDDEN_PATTERNS = [
    r"\bos\.system\b",
    r"\bsubprocess\b",
    r"\beval\s*\(",
    r"\bexec\s*\(",
    r"\b__import__\b",
    r"\bopen\s*\([^)]*['\"]w",  # open with write mode outside safe context
    r"\bcompile\s*\(",
    r"\bglobals\s*\(",
    r"\blocals\s*\(",
]

TOOLS = [
    {
        "name": "list_custom_tools",
        "description": "List all custom tools in ~/.ludolph/custom_tools/",
        "input_schema": {
            "type": "object",
            "properties": {},
        },
    },
    {
        "name": "create_tool",
        "description": "Create a new custom tool. The code must define TOOLS (list) and HANDLERS (dict). Dangerous operations like subprocess, eval, exec are forbidden.",
        "input_schema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Tool name (alphanumeric and underscores only, no .py extension)",
                },
                "code": {
                    "type": "string",
                    "description": "Python code that exports TOOLS list and HANDLERS dict",
                },
            },
            "required": ["name", "code"],
        },
    },
    {
        "name": "delete_tool",
        "description": "Delete a custom tool file",
        "input_schema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Tool name to delete (without .py extension)",
                },
            },
            "required": ["name"],
        },
    },
    {
        "name": "reload_tools",
        "description": "Reload all tools (including newly created custom tools). Sends SIGHUP to the server.",
        "input_schema": {
            "type": "object",
            "properties": {},
        },
    },
]


def _ensure_custom_dir() -> Path:
    """Ensure the custom tools directory exists."""
    CUSTOM_TOOLS_DIR.mkdir(parents=True, exist_ok=True)
    return CUSTOM_TOOLS_DIR


def _validate_tool_name(name: str) -> str | None:
    """Validate tool name. Returns error message or None if valid."""
    if not name:
        return "Tool name is required"
    if not re.match(r"^[a-zA-Z][a-zA-Z0-9_]*$", name):
        return "Tool name must start with a letter and contain only alphanumeric characters and underscores"
    if name.startswith("_"):
        return "Tool name cannot start with underscore"
    return None


def _validate_tool_code(code: str) -> str | None:
    """Validate tool code for security. Returns error message or None if valid."""
    if not code:
        return "Code is required"

    # Check for required exports
    if "TOOLS" not in code:
        return "Code must define TOOLS list"
    if "HANDLERS" not in code:
        return "Code must define HANDLERS dict"

    # Check for forbidden patterns
    for pattern in FORBIDDEN_PATTERNS:
        if re.search(pattern, code):
            return f"Forbidden pattern detected: {pattern}"

    # Try to compile (syntax check)
    try:
        compile(code, "<custom_tool>", "exec")
    except SyntaxError as e:
        return f"Syntax error: {e}"

    return None


def _list_custom_tools(args: dict) -> dict:
    """List all custom tools."""
    custom_dir = _ensure_custom_dir()

    tools = []
    for py_file in sorted(custom_dir.glob("*.py")):
        if py_file.name.startswith("_"):
            continue

        # Try to get basic info
        try:
            content = py_file.read_text(encoding="utf-8")
            # Count tools defined
            tool_count = content.count('"name":') + content.count("'name':")
            tools.append(f"- {py_file.stem} ({tool_count} tool(s))")
        except Exception:
            tools.append(f"- {py_file.stem} (error reading)")

    if not tools:
        return {
            "content": f"No custom tools found in {custom_dir}\n\nUse create_tool to add one.",
            "error": None,
        }

    return {
        "content": f"Custom tools in {custom_dir}:\n\n" + "\n".join(tools),
        "error": None,
    }


def _create_tool(args: dict) -> dict:
    """Create a new custom tool."""
    name = args.get("name", "").strip()
    code = args.get("code", "")

    # Validate name
    name_error = _validate_tool_name(name)
    if name_error:
        return {"content": "", "error": name_error}

    # Validate code
    code_error = _validate_tool_code(code)
    if code_error:
        return {"content": "", "error": code_error}

    custom_dir = _ensure_custom_dir()
    file_path = custom_dir / f"{name}.py"

    # Check if exists
    if file_path.exists():
        return {"content": "", "error": f"Tool '{name}' already exists. Delete it first to replace."}

    # Write the file
    file_path.write_text(code, encoding="utf-8")

    return {
        "content": f"Created custom tool: {file_path}\n\nUse reload_tools to load it.",
        "error": None,
    }


def _delete_tool(args: dict) -> dict:
    """Delete a custom tool."""
    name = args.get("name", "").strip()

    # Validate name
    name_error = _validate_tool_name(name)
    if name_error:
        return {"content": "", "error": name_error}

    custom_dir = _ensure_custom_dir()
    file_path = custom_dir / f"{name}.py"

    if not file_path.exists():
        return {"content": "", "error": f"Tool '{name}' not found"}

    file_path.unlink()

    return {
        "content": f"Deleted custom tool: {name}\n\nUse reload_tools to apply changes.",
        "error": None,
    }


def _reload_tools(args: dict) -> dict:
    """Reload tools by sending SIGHUP to self."""
    try:
        # Send SIGHUP to current process to trigger reload
        os.kill(os.getpid(), signal.SIGHUP)
        return {
            "content": "Reload signal sent. Tools will be reloaded.",
            "error": None,
        }
    except Exception as e:
        return {"content": "", "error": f"Failed to send reload signal: {e}"}


HANDLERS = {
    "list_custom_tools": _list_custom_tools,
    "create_tool": _create_tool,
    "delete_tool": _delete_tool,
    "reload_tools": _reload_tools,
}
