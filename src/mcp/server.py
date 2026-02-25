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
from pathlib import Path

from flask import Flask, jsonify, request

from .security import get_vault_path, init_security, is_git_repo, require_auth
from .tools import call_tool, get_tool_definitions

app = Flask(__name__)

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

    print(f"Vault: {vault_path}")
    print(f"Port: {port}")
    print(f"Git repo: {is_git_repo()}")

    app.run(host="0.0.0.0", port=port)


if __name__ == "__main__":
    main()
