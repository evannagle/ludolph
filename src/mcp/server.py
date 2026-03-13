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
import json
import logging
import os
import signal
import threading
import time
from pathlib import Path

from flask import Flask, Response, jsonify, request

from llm import (
    LlmApiError,
    LlmAuthError,
    LlmBudgetError,
    LlmKeyMissingError,
    LlmRateLimitError,
)
from llm import (
    chat as llm_chat,
    chat_stream as llm_chat_stream,
)
from process_manager import get_process_manager
from registry import Registry
from security import get_vault_path, init_security, is_git_repo, require_auth
from tools import call_tool, get_tool_definitions, reload_tools

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


# Channel for SSE events - thread-safe queue
_event_subscribers: dict[str, list[dict]] = {}
_event_lock = threading.Lock()
_next_event_id = 1


def push_event(event_type: str, data: dict) -> int:
    """Push an event to all subscribers."""
    global _next_event_id
    with _event_lock:
        event_id = _next_event_id
        _next_event_id += 1
        event = {
            "id": event_id,
            "type": event_type,
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "data": data,
        }
        for subscriber_events in _event_subscribers.values():
            subscriber_events.append(event)
        return event_id


def push_channel_message(
    sender: str, content: str, message_id: int, reply_to: int | None = None
):
    """Push a channel message event to SSE subscribers."""
    push_event("channel_message", {
        "from": sender,
        "content": content,
        "id": message_id,
        "reply_to": reply_to,
    })


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


@app.route("/events")
@require_auth
def events():
    """Server-Sent Events endpoint for real-time notifications."""
    subscriber = request.args.get("subscriber", "unknown")

    # Initialize subscriber's event queue
    with _event_lock:
        if subscriber not in _event_subscribers:
            _event_subscribers[subscriber] = []

    logger.info(f"SSE subscriber connected: {subscriber}")

    def generate():
        try:
            last_heartbeat = time.time()
            while True:
                # Check for new events
                with _event_lock:
                    events_to_send = _event_subscribers.get(subscriber, [])
                    _event_subscribers[subscriber] = []

                for event in events_to_send:
                    yield f"data: {json.dumps(event)}\n\n"

                # Send heartbeat every 30 seconds
                if time.time() - last_heartbeat >= 30:
                    heartbeat = {
                        "id": 0,
                        "type": "heartbeat",
                        "timestamp": time.strftime(
                            "%Y-%m-%dT%H:%M:%SZ", time.gmtime()
                        ),
                        "data": {},
                    }
                    yield f"data: {json.dumps(heartbeat)}\n\n"
                    last_heartbeat = time.time()

                time.sleep(0.5)
        finally:
            with _event_lock:
                _event_subscribers.pop(subscriber, None)
            logger.info(f"SSE subscriber disconnected: {subscriber}")

    return Response(generate(), mimetype="text/event-stream")


@app.route("/channel/send", methods=["POST"])
@require_auth
def channel_send():
    """Send a message to the channel via SSE.

    This endpoint receives messages from Claude Code and pushes them
    as SSE events to connected Pi clients.

    Request body:
        from: Sender identifier (e.g., "claude_code")
        content: Message content
        reply_to: Optional message ID being replied to
    """
    data = request.get_json()
    if not data:
        return jsonify({"error": "JSON body required"}), 400

    sender = data.get("from", "unknown")
    content = data.get("content", "")
    reply_to = data.get("reply_to")

    if not content:
        return jsonify({"error": "content is required"}), 400

    # Generate message ID (simple incrementing for now)
    global _next_event_id
    with _event_lock:
        message_id = _next_event_id

    # Push SSE event to all subscribers
    push_channel_message(sender, content, message_id, reply_to)

    return jsonify({
        "status": "sent",
        "id": message_id,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    })


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


def transform_messages_for_openai(messages: list) -> list:
    """
    Transform Anthropic-style messages to OpenAI-style for LiteLLM.

    Anthropic uses content blocks: [{"type": "tool_use", ...}]
    OpenAI uses tool_calls field on assistant messages and role="tool" for results.
    """
    result = []

    for msg in messages:
        role = msg.get("role", "")
        content = msg.get("content", "")

        # String content passes through unchanged
        if isinstance(content, str):
            result.append(msg)
            continue

        # List content needs transformation
        if not isinstance(content, list):
            result.append(msg)
            continue

        # Check what types of blocks we have
        block_types = {b.get("type") for b in content if isinstance(b, dict)}

        # Assistant message with tool_use -> extract tool_calls
        if role == "assistant" and "tool_use" in block_types:
            for block in content:
                if block.get("type") == "tool_use":
                    tool_calls = block.get("tool_calls", [])
                    if tool_calls:
                        result.append({
                            "role": "assistant",
                            "content": None,
                            "tool_calls": tool_calls,
                        })
                    break
            continue

        # User message with tool_result -> convert to tool role messages
        if role == "user" and "tool_result" in block_types:
            for block in content:
                if block.get("type") == "tool_result":
                    result.append({
                        "role": "tool",
                        "tool_call_id": block.get("tool_use_id", ""),
                        "content": block.get("content", ""),
                    })
            continue

        # Unknown format - pass through
        result.append(msg)

    return result


@app.route("/chat", methods=["POST"])
@require_auth
def chat():
    """Proxy chat request to LLM provider via LiteLLM."""
    data = request.json or {}
    model = data.get("model", "claude-sonnet-4-20250514")
    messages = data.get("messages", [])
    tools = data.get("tools")

    # Validate required fields
    if not messages or not isinstance(messages, list):
        return jsonify({"error": "invalid_input", "message": "messages must be a non-empty list"}), 400

    # Transform Anthropic-style messages to OpenAI-style for LiteLLM
    transformed_messages = transform_messages_for_openai(messages)

    try:
        result = llm_chat(model=model, messages=transformed_messages, tools=tools)
        return jsonify(result)
    except LlmKeyMissingError as e:
        return jsonify({"error": "api_key_missing", "message": str(e)}), 401
    except LlmAuthError as e:
        return jsonify({"error": "auth_failed", "message": str(e)}), 401
    except LlmBudgetError as e:
        return jsonify({"error": "budget_exceeded", "message": str(e)}), 402
    except LlmRateLimitError as e:
        return jsonify({"error": "rate_limit", "message": str(e)}), 429
    except LlmApiError as e:
        return jsonify({"error": "api_error", "message": str(e)}), 502
    except Exception as e:
        logger.exception("Unexpected error in /chat endpoint")
        return jsonify({"error": "internal_error", "message": str(e)}), 500


@app.route("/chat/stream", methods=["POST"])
@require_auth
def chat_stream():
    """Stream chat response via Server-Sent Events."""
    data = request.json or {}
    model = data.get("model", "claude-sonnet-4-20250514")
    messages = data.get("messages", [])
    tools = data.get("tools")

    if not messages or not isinstance(messages, list):
        return jsonify({"error": "invalid_input", "message": "messages must be a non-empty list"}), 400

    def generate():
        try:
            for chunk in llm_chat_stream(model=model, messages=messages, tools=tools):
                yield f"data: {json.dumps(chunk)}\n\n"
            yield "data: [DONE]\n\n"
        except LlmKeyMissingError as e:
            yield f"data: {json.dumps({'error': 'api_key_missing', 'message': str(e)})}\n\n"
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
# Admin Endpoints
# -----------------------------------------------------------------------------


@app.route("/admin/health", methods=["GET"])
@require_auth
def admin_health():
    """
    Test API key health by making a minimal LLM call.

    Returns:
        JSON with api_key_valid, error message if invalid
    """
    import os

    api_key = os.environ.get("ANTHROPIC_API_KEY", "")

    if not api_key:
        return jsonify({
            "api_key_valid": False,
            "error": "No API key configured",
            "fix": "Run install-mcp.sh or set ANTHROPIC_API_KEY",
        })

    # Test with minimal API call
    try:
        result = llm_chat(
            model="claude-sonnet-4-20250514",
            messages=[{"role": "user", "content": "hi"}],
        )
        return jsonify({
            "api_key_valid": True,
            "model": "claude-sonnet-4-20250514",
        })
    except LlmAuthError as e:
        return jsonify({
            "api_key_valid": False,
            "error": "API key is invalid or expired",
            "fix": "Get a new key from console.anthropic.com/account/keys",
        })
    except Exception as e:
        error_str = str(e).lower()
        # Parse common errors into user-friendly messages
        if "credit balance" in error_str or "budget" in error_str:
            return jsonify({
                "api_key_valid": True,  # Key is valid, just no credits
                "error": "API credits exhausted",
                "fix": "Add credits at console.anthropic.com/settings/billing",
            })
        elif "rate limit" in error_str:
            return jsonify({
                "api_key_valid": True,
                "error": "Rate limited",
                "fix": "Wait a moment and try again",
            })
        else:
            return jsonify({
                "api_key_valid": False,
                "error": str(e),
                "fix": "Check MCP server logs for details",
            })


@app.route("/admin/update-api-key", methods=["POST"])
@require_auth
def admin_update_api_key():
    """
    Update the Anthropic API key in the launchd plist.

    Body:
        api_key: The new API key (must start with sk-ant-)

    This updates the plist file and tells the caller to restart the service.
    The service must be restarted separately for the change to take effect.
    """
    import subprocess

    data = request.json or {}
    new_key = data.get("api_key", "").strip()

    # Validate key format
    if not new_key:
        return jsonify({"error": "api_key is required"}), 400

    if not new_key.startswith("sk-ant-"):
        return jsonify({"error": "Invalid key format. Must start with sk-ant-"}), 400

    # Test the new key before saving
    import os
    old_key = os.environ.get("ANTHROPIC_API_KEY", "")
    os.environ["ANTHROPIC_API_KEY"] = new_key

    try:
        result = llm_chat(
            model="claude-sonnet-4-20250514",
            messages=[{"role": "user", "content": "hi"}],
        )
    except Exception as e:
        # Restore old key
        os.environ["ANTHROPIC_API_KEY"] = old_key
        return jsonify({
            "error": "New API key is invalid",
            "details": str(e),
        }), 400

    # Key is valid - update the plist
    plist_path = Path.home() / "Library/LaunchAgents/dev.ludolph.mcp.plist"

    if not plist_path.exists():
        return jsonify({"error": "Launchd plist not found"}), 500

    try:
        subprocess.run(
            ["plutil", "-replace", "EnvironmentVariables.ANTHROPIC_API_KEY",
             "-string", new_key, str(plist_path)],
            check=True,
            capture_output=True,
        )
    except subprocess.CalledProcessError as e:
        return jsonify({
            "error": "Failed to update plist",
            "details": e.stderr.decode() if e.stderr else str(e),
        }), 500

    return jsonify({
        "status": "ok",
        "message": "API key updated. Restart the MCP service to apply.",
        "restart_command": "launchctl kickstart -k gui/$(id -u)/dev.ludolph.mcp",
    })


# -----------------------------------------------------------------------------
# Plugin Management Endpoints
# -----------------------------------------------------------------------------

# Lazy-loaded plugin manager
_plugin_manager = None


def get_plugin_manager():
    """Get or create the plugin manager instance."""
    global _plugin_manager
    if _plugin_manager is None:
        from plugins import PluginManager
        _plugin_manager = PluginManager()
    return _plugin_manager


@app.route("/plugin/search", methods=["GET"])
@require_auth
def plugin_search():
    """Search for plugins in the community registry."""
    query = request.args.get("q", "")

    # For now, return empty results - registry lookup not yet implemented
    # In the future, this would query github.com/ludolph-community
    return jsonify({
        "query": query,
        "plugins": [],
        "message": "Registry search not yet implemented. Use git URLs to install plugins.",
    })


@app.route("/plugin/install", methods=["POST"])
@require_auth
def plugin_install():
    """Install a plugin from source."""
    from plugins import PluginInstallError

    data = request.json or {}
    source = data.get("source", "")

    if not source:
        return jsonify({"error": "source is required"}), 400

    manager = get_plugin_manager()

    try:
        manifest = manager.install(source)
        return jsonify({
            "status": "ok",
            "name": manifest.name,
            "version": manifest.version,
            "description": manifest.description,
            "needs_setup": manager.needs_setup(manifest.name),
            "tools": [t.name for t in manifest.tools],
        })
    except PluginInstallError as e:
        return jsonify({"error": str(e)}), 400
    except Exception as e:
        logger.exception(f"Plugin install failed: {e}")
        return jsonify({"error": f"Install failed: {e}"}), 500


@app.route("/plugin/list", methods=["GET"])
@require_auth
def plugin_list():
    """List installed plugins."""
    manager = get_plugin_manager()
    plugins = manager.list()

    return jsonify({
        "plugins": [
            {
                "name": p.name,
                "version": p.version,
                "description": p.description,
                "enabled": p.enabled,
            }
            for p in plugins
        ]
    })


@app.route("/plugin/<name>/enable", methods=["POST"])
@require_auth
def plugin_enable_endpoint(name: str):
    """Enable a plugin."""
    manager = get_plugin_manager()

    if manager.enable(name):
        return jsonify({"status": "ok", "name": name, "enabled": True})
    else:
        return jsonify({"error": "Plugin not found"}), 404


@app.route("/plugin/<name>/disable", methods=["POST"])
@require_auth
def plugin_disable_endpoint(name: str):
    """Disable a plugin."""
    manager = get_plugin_manager()

    if manager.disable(name):
        return jsonify({"status": "ok", "name": name, "enabled": False})
    else:
        return jsonify({"error": "Plugin not found"}), 404


@app.route("/plugin/<name>/remove", methods=["POST"])
@require_auth
def plugin_remove_endpoint(name: str):
    """Remove a plugin."""
    manager = get_plugin_manager()

    if manager.remove(name):
        return jsonify({"status": "ok", "name": name, "removed": True})
    else:
        return jsonify({"error": "Plugin not found"}), 404


@app.route("/plugin/<name>/check", methods=["GET"])
@require_auth
def plugin_check_endpoint(name: str):
    """Run health check on a plugin."""
    from plugins import PluginNotFoundError

    manager = get_plugin_manager()

    try:
        result = manager.check(name)
        return jsonify(result)
    except PluginNotFoundError:
        return jsonify({"error": "Plugin not found"}), 404


@app.route("/plugin/<name>/credentials", methods=["GET"])
@require_auth
def plugin_credentials_get(name: str):
    """Get credential requirements for a plugin."""
    from plugins import PluginNotFoundError

    manager = get_plugin_manager()

    try:
        manifest = manager.get(name)
        return jsonify({
            "name": name,
            "credentials": [
                {
                    "name": c.name,
                    "description": c.description,
                    "required": c.required,
                    "oauth_flow": c.oauth_flow,
                }
                for c in manifest.credentials
            ],
        })
    except PluginNotFoundError:
        return jsonify({"error": "Plugin not found"}), 404


@app.route("/plugin/<name>/credentials", methods=["POST"])
@require_auth
def plugin_credentials_set(name: str):
    """Set credentials for a plugin."""
    from plugins import PluginNotFoundError

    manager = get_plugin_manager()
    storage = manager.storage

    try:
        manifest = manager.get(name)
    except PluginNotFoundError:
        return jsonify({"error": "Plugin not found"}), 404

    data = request.json or {}

    # Validate that all provided credentials are valid for this plugin
    valid_creds = {c.name for c in manifest.credentials}
    for key in data.keys():
        if key not in valid_creds:
            return jsonify({"error": f"Unknown credential: {key}"}), 400

    # Save credentials
    for key, value in data.items():
        if value:  # Don't save empty values
            storage.set_credential(name, key, value)

    return jsonify({
        "status": "ok",
        "name": name,
        "saved": list(data.keys()),
    })


@app.route("/plugin/<name>/update", methods=["POST"])
@require_auth
def plugin_update_single(name: str):
    """Update a single plugin."""
    from plugins import PluginNotFoundError

    manager = get_plugin_manager()

    try:
        result = manager.update(name)
        if result:
            return jsonify({
                "status": "ok",
                "name": name,
                "version": result.version,
                "updated": True,
            })
        else:
            return jsonify({
                "status": "ok",
                "name": name,
                "updated": False,
                "message": "Already up to date",
            })
    except PluginNotFoundError:
        return jsonify({"error": "Plugin not found"}), 404


@app.route("/plugin/update", methods=["POST"])
@require_auth
def plugin_update_all():
    """Update all plugins."""
    manager = get_plugin_manager()
    updated = manager.update_all()

    return jsonify({
        "status": "ok",
        "updated": [
            {"name": m.name, "version": m.version}
            for m in updated
        ],
    })


@app.route("/plugin/<name>/logs", methods=["GET"])
@require_auth
def plugin_logs_endpoint(name: str):
    """Get plugin logs."""
    from plugins import PluginNotFoundError

    manager = get_plugin_manager()
    lines = int(request.args.get("lines", 20))

    try:
        manager.get(name)  # Verify plugin exists
    except PluginNotFoundError:
        return jsonify({"error": "Plugin not found"}), 404

    # For now, logs are not implemented - would need to capture MCP process output
    return jsonify({
        "name": name,
        "logs": "",
        "message": "Plugin logging not yet implemented",
    })


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
    # Configure logging
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )

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
