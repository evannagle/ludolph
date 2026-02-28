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

import os
import signal
from pathlib import Path

from flask import Flask, jsonify, request

from .llm import (
    LlmApiError,
    LlmAuthError,
    LlmBudgetError,
    LlmRateLimitError,
)
from .llm import (
    chat as llm_chat,
)
from .security import get_vault_path, init_security, is_git_repo, require_auth
from .tools import call_tool, get_tool_definitions, reload_tools

app = Flask(__name__)


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


@app.route("/tools")
@require_auth
def tools():
    """Return available tool definitions."""
    return jsonify({"tools": get_tool_definitions()})


@app.route("/tools/call", methods=["POST"])
@require_auth
def tools_call():
    """Execute a tool and return the result."""
    data = request.json or {}
    name = data.get("name", "")
    arguments = data.get("arguments", {})

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


def main():
    """Run the server."""
    vault_path = Path(os.environ.get("VAULT_PATH", "~/vault")).expanduser().resolve()
    auth_token = os.environ.get("AUTH_TOKEN", "")
    port = int(os.environ.get("PORT", 8200))

    if not auth_token:
        print("Warning: AUTH_TOKEN not set - server is unprotected!")

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
