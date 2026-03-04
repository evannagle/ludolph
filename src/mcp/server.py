#!/usr/bin/env python3
"""
Ludolph MCP Server - General-purpose filesystem access via HTTP.

Provides read/write access to any folder structure. Works with Obsidian vaults,
code repositories, or any directory. Git-aware: respects .gitignore when present.

Usage:
    VAULT_PATH=/path/to/folder AUTH_TOKEN=secret python server.py

Environment Variables:
    VAULT_PATH: Root directory for file operations (required)
    AUTH_TOKEN: Bearer token for authentication (required for security)
    PORT: Server port (default: 8200)
"""

import asyncio
import logging
import os
import signal
from pathlib import Path

import json

from flask import Flask, Response, jsonify, request

from .llm import (
    LlmApiError,
    LlmAuthError,
    LlmBudgetError,
    LlmRateLimitError,
)
from .llm import (
    chat as llm_chat,
    chat_stream as llm_chat_stream,
)
from .event_bus import get_event_bus
from .process_manager import get_process_manager
from .registry import Registry
from .security import get_vault_path, init_security, is_git_repo, require_auth
from .tools import call_tool, get_tool_definitions, reload_tools

logger = logging.getLogger(__name__)

app = Flask(__name__)

# MCP Registry paths
MCPS_PATH = Path.home() / ".ludolph" / "mcps"
REGISTRY_PATH = MCPS_PATH / "registry.toml"
USERS_PATH = MCPS_PATH / "users"

# Initialize registry (lazy loaded to allow paths to be configured)
_registry: Registry | None = None


def get_registry() -> Registry:
    """Get or create the MCP registry instance."""
    global _registry
    if _registry is None:
        _registry = Registry(REGISTRY_PATH, USERS_PATH)
    return _registry


# External MCP tool naming convention: {mcp_name}__{tool_name}
EXTERNAL_TOOL_SEPARATOR = "__"


def parse_external_tool_name(name: str) -> tuple[str, str] | None:
    """
    Parse an external MCP tool name into (mcp_name, tool_name).

    External tools use double underscore separator: slack__send_message

    Args:
        name: Tool name to parse

    Returns:
        Tuple of (mcp_name, tool_name) if external, None if builtin
    """
    if EXTERNAL_TOOL_SEPARATOR not in name:
        return None

    parts = name.split(EXTERNAL_TOOL_SEPARATOR, 1)
    if len(parts) != 2:
        return None

    mcp_name, tool_name = parts
    if not mcp_name or not tool_name:
        return None

    return (mcp_name, tool_name)


async def _call_external_tool(
    mcp_name: str,
    tool_name: str,
    arguments: dict,
    user_id: int | None = None,
) -> dict:
    """
    Call a tool on an external MCP.

    Args:
        mcp_name: Name of the external MCP (e.g., "slack")
        tool_name: Name of the tool on that MCP (e.g., "send_message")
        arguments: Arguments to pass to the tool
        user_id: Optional user ID for credential lookup

    Returns:
        Result dict with 'content' and optional 'error' keys
    """
    registry = get_registry()
    defn = registry.get_definition(mcp_name)

    if defn is None:
        return {"content": "", "error": f"Unknown MCP: {mcp_name}"}

    if defn.type != "external":
        return {"content": "", "error": f"MCP '{mcp_name}' is not external"}

    if not defn.package:
        return {"content": "", "error": f"MCP '{mcp_name}' has no package defined"}

    # Build environment from user credentials if available
    env = {}
    if user_id is not None:
        user_config = registry.get_user_config(user_id)
        user_env = user_config.get("env", {})
        for var in defn.env_vars:
            if var in user_env:
                env[var] = user_env[var]

    # Get or spawn the MCP process
    manager = get_process_manager()
    try:
        mcp_proc = await manager.get_or_spawn(mcp_name, defn.package, env if env else None)
        result = await manager.call_tool(mcp_proc, tool_name, arguments)

        # MCP returns {content: [...], isError: bool}
        # Normalize to our format
        content_items = result.get("content", [])
        if isinstance(content_items, list):
            # Extract text from content items
            texts = []
            for item in content_items:
                if isinstance(item, dict) and item.get("type") == "text":
                    texts.append(item.get("text", ""))
                elif isinstance(item, str):
                    texts.append(item)
            content = "\n".join(texts)
        else:
            content = str(content_items)

        if result.get("isError"):
            return {"content": content, "error": content}

        return {"content": content, "error": None}

    except RuntimeError as e:
        logger.error(f"External MCP call failed: {e}")
        return {"content": "", "error": str(e)}
    except Exception as e:
        logger.exception(f"Unexpected error calling external MCP: {e}")
        return {"content": "", "error": f"Internal error: {e}"}


def _handle_sighup(signum, frame):
    """Handle SIGHUP to hot-reload custom tools."""
    reload_tools()

# Read version from VERSION file (populated during release)
VERSION_FILE = Path(__file__).parent / "VERSION"
VERSION = VERSION_FILE.read_text().strip() if VERSION_FILE.exists() else "dev"


@app.route("/")
def root():
    """Server info (no auth required)."""
    return jsonify({"name": "Ludolph MCP Server", "version": VERSION, "status": "running"})


@app.route("/health")
@require_auth
def health():
    """Health check endpoint."""
    vault = get_vault_path()
    return jsonify({"status": "ok", "vault": str(vault), "git_repo": is_git_repo()})


@app.route("/status")
@require_auth
def status():
    """Return server status with simplified tool list."""
    tool_defs = get_tool_definitions()
    tools_summary = [
        {"name": t["name"], "description": t.get("description", "")}
        for t in tool_defs
    ]
    return jsonify({
        "status": "ok",
        "tools": tools_summary,
        "version": VERSION,
    })


@app.route("/tools")
@require_auth
def tools():
    """Return available tool definitions."""
    return jsonify({"tools": get_tool_definitions()})


@app.route("/tools/call", methods=["POST"])
@require_auth
def tools_call():
    """
    Execute a tool and return the result.

    For external MCP tools, use naming convention: {mcp_name}__{tool_name}
    Example: slack__send_message, github__list_repos

    Optional body fields:
        user_id: Telegram user ID (for credential lookup on external MCPs)
    """
    data = request.json or {}
    name = data.get("name", "")
    arguments = data.get("arguments", {})
    user_id = data.get("user_id")

    # Check if this is an external MCP tool
    external_parts = parse_external_tool_name(name)

    if external_parts:
        mcp_name, tool_name = external_parts
        # Use asyncio.run() to bridge sync Flask with async ProcessManager
        result = asyncio.run(_call_external_tool(mcp_name, tool_name, arguments, user_id))
    else:
        # Builtin tool
        result = call_tool(name, arguments)

    return jsonify(result)


@app.route("/chat", methods=["POST"])
@require_auth
def chat():
    """Proxy chat request to LLM provider via LiteLLM."""
    data = request.json or {}
    model = data.get("model", "claude-sonnet-4")
    messages = data.get("messages", [])
    tools = data.get("tools")

    # Validate required fields
    if not messages or not isinstance(messages, list):
        return jsonify({"error": "invalid_input", "message": "messages must be a non-empty list"}), 400

    try:
        result = llm_chat(model=model, messages=messages, tools=tools)
        return jsonify(result)
    except LlmAuthError as e:
        return jsonify({"error": "auth_failed", "message": str(e)}), 401
    except LlmBudgetError as e:
        return jsonify({"error": "budget_exceeded", "message": str(e)}), 402
    except LlmRateLimitError as e:
        return jsonify({"error": "rate_limit", "message": str(e)}), 429
    except LlmApiError as e:
        return jsonify({"error": "api_error", "message": str(e)}), 502
    except Exception as e:
        return jsonify({"error": "internal_error", "message": "An unexpected error occurred"}), 500


@app.route("/chat/stream", methods=["POST"])
@require_auth
def chat_stream():
    """Stream chat response via Server-Sent Events."""
    data = request.json or {}
    model = data.get("model", "claude-sonnet-4")
    messages = data.get("messages", [])
    tools = data.get("tools")

    if not messages or not isinstance(messages, list):
        return jsonify({"error": "invalid_input", "message": "messages must be a non-empty list"}), 400

    def generate():
        try:
            for chunk in llm_chat_stream(model=model, messages=messages, tools=tools):
                yield f"data: {json.dumps(chunk)}\n\n"
            yield "data: [DONE]\n\n"
        except LlmAuthError as e:
            yield f"data: {json.dumps({'error': 'auth_failed', 'message': str(e)})}\n\n"
        except LlmBudgetError as e:
            yield f"data: {json.dumps({'error': 'budget_exceeded', 'message': str(e)})}\n\n"
        except LlmRateLimitError as e:
            yield f"data: {json.dumps({'error': 'rate_limit', 'message': str(e)})}\n\n"
        except LlmApiError as e:
            yield f"data: {json.dumps({'error': 'api_error', 'message': str(e)})}\n\n"

    return Response(generate(), mimetype="text/event-stream")


# -----------------------------------------------------------------------------
# MCP Management Endpoints
# -----------------------------------------------------------------------------


@app.route("/mcp/registry", methods=["GET"])
@require_auth
def list_registry():
    """List all available MCPs from registry."""
    registry = get_registry()
    mcps = registry.list_available()

    return jsonify({
        "mcps": [
            {
                "name": mcp.name,
                "description": mcp.description,
                "type": mcp.type,
                "package": mcp.package,
                "env_vars": mcp.env_vars,
            }
            for mcp in mcps
        ]
    })


@app.route("/mcp/user/<int:user_id>", methods=["GET"])
@require_auth
def get_user_mcps(user_id: int):
    """Get user's enabled MCPs."""
    registry = get_registry()
    enabled = registry.get_user_enabled_mcps(user_id)

    return jsonify({"user_id": user_id, "enabled": enabled})


@app.route("/mcp/user/<int:user_id>/enable/<name>", methods=["POST"])
@require_auth
def enable_mcp(user_id: int, name: str):
    """Enable an MCP for a user."""
    registry = get_registry()

    # Get credentials from request body if provided
    data = request.json or {}
    credentials = data.get("credentials")

    success = registry.enable_mcp(user_id, name, credentials)

    if not success:
        return jsonify({
            "error": "not_found",
            "message": f"MCP '{name}' not found in registry"
        }), 404

    return jsonify({
        "status": "ok",
        "user_id": user_id,
        "mcp": name,
        "enabled": True
    })


@app.route("/mcp/user/<int:user_id>/disable/<name>", methods=["POST"])
@require_auth
def disable_mcp(user_id: int, name: str):
    """Disable an MCP for a user."""
    registry = get_registry()

    success = registry.disable_mcp(user_id, name)

    if not success:
        return jsonify({
            "error": "not_found",
            "message": f"MCP '{name}' not found in registry"
        }), 404

    return jsonify({
        "status": "ok",
        "user_id": user_id,
        "mcp": name,
        "enabled": False
    })


# -----------------------------------------------------------------------------
# Event Bus SSE Endpoint
# -----------------------------------------------------------------------------


@app.route("/events", methods=["GET"])
@require_auth
def events():
    """
    SSE stream of events for a subscriber.

    Query parameters:
        subscriber: Unique identifier for this subscriber (required)

    Returns:
        Server-Sent Events stream with events from the event bus.
        Each event is JSON-encoded in the data field.
        Keepalive comments are sent periodically.
    """
    subscriber = request.args.get("subscriber")
    if not subscriber:
        return jsonify({"error": "subscriber parameter required"}), 400

    bus = get_event_bus()
    bus.subscribe(subscriber)

    def generate():
        import time

        last_id = 0
        while True:
            events_list = bus.receive(subscriber, since_id=last_id)
            for event in events_list:
                last_id = event.id
                data = {
                    "id": event.id,
                    "type": event.type,
                    "timestamp": event.timestamp,
                    "data": event.data,
                }
                yield f"data: {json.dumps(data)}\n\n"

            # Keepalive comment (SSE spec: lines starting with : are comments)
            yield ": keepalive\n\n"
            time.sleep(1)

    return Response(generate(), mimetype="text/event-stream")


def _load_env_file():
    """Load .env file if it exists."""
    env_file = Path(__file__).parent / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, value = line.split("=", 1)
                key = key.strip()
                value = value.strip().strip('"').strip("'")
                if key not in os.environ:  # Don't override existing env vars
                    os.environ[key] = value


def main():
    """Run the server."""
    # Load .env file first
    _load_env_file()

    vault_path = Path(os.environ.get("VAULT_PATH", "~/vault")).expanduser().resolve()
    auth_token = os.environ.get("AUTH_TOKEN", "")
    api_key = os.environ.get("ANTHROPIC_API_KEY", "")
    port = int(os.environ.get("PORT", 8200))

    if not auth_token:
        print("Warning: AUTH_TOKEN not set - server is unprotected!")

    if not api_key:
        print("Warning: ANTHROPIC_API_KEY not set - LLM proxy won't work!")
        print("Run 'python -m setup_llm' to configure credentials.")

    if not vault_path.exists():
        print(f"Warning: Vault path does not exist: {vault_path}")

    # Initialize security module
    init_security(vault_path, auth_token)

    # Register SIGHUP handler for hot-reloading custom tools
    signal.signal(signal.SIGHUP, _handle_sighup)

    print(f"Vault: {vault_path}")
    print(f"Port: {port}")
    print(f"Git repo: {is_git_repo()}")

    app.run(host="0.0.0.0", port=port)


if __name__ == "__main__":
    main()
