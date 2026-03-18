"""Meta-tools for dynamic tool creation and management.

Allows Lu to create, list, and delete skills at runtime.
Skills are stored in ~/.ludolph/skills/ and auto-loaded.
"""

import os
import re
import signal
from pathlib import Path

# Skills directory
SKILLS_DIR = Path.home() / ".ludolph" / "skills"

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
        "name": "list_skills",
        "description": "List all skills in ~/.ludolph/skills/",
        "input_schema": {
            "type": "object",
            "properties": {},
        },
    },
    {
        "name": "create_skill",
        "description": "Create a new skill. Code must define TOOLS list (each with 'name', 'description', 'input_schema' in snake_case) and HANDLERS dict. IMPORTANT: Use 'input_schema' NOT 'parameters' or 'inputSchema'. Subprocess/eval/exec forbidden.",
        "input_schema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name (alphanumeric and underscores only, no .py extension)",
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
        "name": "delete_skill",
        "description": "Delete a skill file",
        "input_schema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name to delete (without .py extension)",
                },
            },
            "required": ["name"],
        },
    },
    {
        "name": "reload_tools",
        "description": "Reload all tools (including newly created skills). Sends SIGHUP to the server.",
        "input_schema": {
            "type": "object",
            "properties": {},
        },
    },
    {
        "name": "complete_setup",
        "description": "Signal that setup is complete. Call this after writing Lu.md to exit setup mode.",
        "input_schema": {
            "type": "object",
            "properties": {},
        },
    },
]


def _ensure_skills_dir() -> Path:
    """Ensure the skills directory exists."""
    SKILLS_DIR.mkdir(parents=True, exist_ok=True)
    return SKILLS_DIR


def _validate_skill_name(name: str) -> str | None:
    """Validate skill name. Returns error message or None if valid."""
    if not name:
        return "Skill name is required"
    if not re.match(r"^[a-zA-Z][a-zA-Z0-9_]*$", name):
        return "Skill name must start with a letter and contain only alphanumeric characters and underscores"
    if name.startswith("_"):
        return "Skill name cannot start with underscore"
    return None


def _validate_skill_code(code: str) -> str | None:
    """Validate skill code for security and correctness. Returns error message or None if valid."""
    if not code:
        return "Code is required"

    # Check for required exports
    if "TOOLS" not in code:
        return "Code must define TOOLS list"
    if "HANDLERS" not in code:
        return "Code must define HANDLERS dict"

    # Check for common schema mistakes (must use snake_case 'input_schema')
    if '"parameters"' in code or "'parameters'" in code:
        return (
            "Error: Use 'input_schema' not 'parameters'\n\n"
            "Example:\n"
            "TOOLS = [{\n"
            "    'name': 'my_tool',\n"
            "    'description': 'Does something',\n"
            "    'input_schema': {  # NOT 'parameters'\n"
            "        'type': 'object',\n"
            "        'properties': {...}\n"
            "    }\n"
            "}]"
        )
    if '"inputSchema"' in code or "'inputSchema'" in code:
        return (
            "Error: Use 'input_schema' (snake_case) not 'inputSchema' (camelCase)\n\n"
            "Example:\n"
            "TOOLS = [{\n"
            "    'name': 'my_tool',\n"
            "    'description': 'Does something',\n"
            "    'input_schema': {  # NOT 'inputSchema'\n"
            "        'type': 'object',\n"
            "        'properties': {...}\n"
            "    }\n"
            "}]"
        )
    if "input_schema" not in code:
        return (
            "Error: TOOLS must include 'input_schema' for each tool\n\n"
            "Example:\n"
            "TOOLS = [{\n"
            "    'name': 'my_tool',\n"
            "    'description': 'Does something',\n"
            "    'input_schema': {\n"
            "        'type': 'object',\n"
            "        'properties': {\n"
            "            'arg1': {'type': 'string', 'description': 'First argument'}\n"
            "        },\n"
            "        'required': ['arg1']\n"
            "    }\n"
            "}]"
        )

    # Check for forbidden patterns
    for pattern in FORBIDDEN_PATTERNS:
        if re.search(pattern, code):
            return f"Forbidden pattern detected: {pattern}"

    # Try to compile (syntax check)
    try:
        compile(code, "<skill>", "exec")
    except SyntaxError as e:
        return f"Syntax error: {e}"

    return None


def _list_skills(args: dict) -> dict:
    """List all skills."""
    skills_dir = _ensure_skills_dir()

    skills = []
    for py_file in sorted(skills_dir.glob("*.py")):
        if py_file.name.startswith("_"):
            continue

        # Try to get basic info
        try:
            content = py_file.read_text(encoding="utf-8")
            # Count tools defined
            tool_count = content.count('"name":') + content.count("'name':")
            skills.append(f"- {py_file.stem} ({tool_count} tool(s))")
        except Exception:
            skills.append(f"- {py_file.stem} (error reading)")

    if not skills:
        return {
            "content": f"No skills found in {skills_dir}\n\nUse create_skill to add one.",
            "error": None,
        }

    return {
        "content": f"Skills in {skills_dir}:\n\n" + "\n".join(skills),
        "error": None,
    }


def _create_skill(args: dict) -> dict:
    """Create a new skill."""
    name = args.get("name", "").strip()
    code = args.get("code", "")

    # Validate name
    name_error = _validate_skill_name(name)
    if name_error:
        return {"content": "", "error": name_error}

    # Validate code
    code_error = _validate_skill_code(code)
    if code_error:
        return {"content": "", "error": code_error}

    skills_dir = _ensure_skills_dir()
    file_path = skills_dir / f"{name}.py"

    # Check if exists
    if file_path.exists():
        return {
            "content": "",
            "error": f"Skill '{name}' already exists. Delete it first to replace.",
        }

    # Write the file
    file_path.write_text(code, encoding="utf-8")

    return {
        "content": f"Created skill: {file_path}\n\nUse reload_tools to load it.",
        "error": None,
    }


def _delete_skill(args: dict) -> dict:
    """Delete a skill."""
    name = args.get("name", "").strip()

    # Validate name
    name_error = _validate_skill_name(name)
    if name_error:
        return {"content": "", "error": name_error}

    skills_dir = _ensure_skills_dir()
    file_path = skills_dir / f"{name}.py"

    if not file_path.exists():
        return {"content": "", "error": f"Skill '{name}' not found"}

    file_path.unlink()

    return {
        "content": f"Deleted skill: {name}\n\nUse reload_tools to apply changes.",
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


def _complete_setup(args: dict) -> dict:
    """Signal setup completion.

    Returns a marker that the bot uses to detect setup completion.
    """
    return {
        "content": "[SETUP_COMPLETE] Setup wizard completed successfully.",
        "error": None,
    }


HANDLERS = {
    "list_skills": _list_skills,
    "create_skill": _create_skill,
    "delete_skill": _delete_skill,
    "reload_tools": _reload_tools,
    "complete_setup": _complete_setup,
}
