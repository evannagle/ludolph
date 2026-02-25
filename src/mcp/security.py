"""Security utilities for path validation and authentication."""

import os
from functools import wraps
from pathlib import Path

from flask import jsonify, request

# Global vault path - set by server initialization
_VAULT_PATH: Path | None = None
_AUTH_TOKEN: str = ""


def init_security(vault_path: Path, auth_token: str) -> None:
    """Initialize security module with vault path and auth token."""
    global _VAULT_PATH, _AUTH_TOKEN
    _VAULT_PATH = vault_path.resolve()
    _AUTH_TOKEN = auth_token


def get_vault_path() -> Path:
    """Get the configured vault path."""
    if _VAULT_PATH is None:
        raise RuntimeError("Security module not initialized")
    return _VAULT_PATH


def safe_path(relative: str) -> Path | None:
    """
    Resolve a path safely within the vault, preventing directory traversal.

    Args:
        relative: Path relative to vault root

    Returns:
        Resolved absolute path if safe, None if path escapes vault
    """
    vault = get_vault_path()

    # Reject any path containing ..
    if ".." in relative:
        return None

    # Handle empty path as vault root
    if not relative or relative == ".":
        return vault

    # Resolve and verify containment
    full = (vault / relative).resolve()

    try:
        full.relative_to(vault)
        return full
    except ValueError:
        return None


def require_auth(f):
    """Decorator to require Bearer token authentication."""
    @wraps(f)
    def decorated(*args, **kwargs):
        auth = request.headers.get("Authorization", "")
        if auth != f"Bearer {_AUTH_TOKEN}":
            return jsonify({"error": "Unauthorized"}), 401
        return f(*args, **kwargs)
    return decorated


def is_git_ignored(path: Path) -> bool:
    """
    Check if a path is git-ignored.

    Returns False if not in a git repo or if git is not available.
    """
    import subprocess

    try:
        result = subprocess.run(
            ["git", "check-ignore", "-q", str(path)],
            cwd=get_vault_path(),
            capture_output=True,
            timeout=5,
        )
        return result.returncode == 0
    except (subprocess.SubprocessError, FileNotFoundError):
        return False


def is_git_repo() -> bool:
    """Check if the vault is inside a git repository."""
    vault = get_vault_path()
    return (vault / ".git").is_dir() or any(
        (p / ".git").is_dir() for p in vault.parents
    )
