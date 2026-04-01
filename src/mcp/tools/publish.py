"""Publish tools — share vault knowledge profiles with the community.

Generates a vault profile showing what the vault knows (not what it
contains) and publishes it to the ludolph registry on GitHub.
"""

import json
import logging
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

REGISTRY_REPO = "evannagle/ludolph-registry"
PROFILE_PATH = Path.home() / ".ludolph" / "vault-profile.json"


def _generate_profile(args: dict[str, Any]) -> dict:
    """Generate a vault profile for publishing."""
    from tools.embeddings import get_store

    name = args.get("name", "").strip()
    description = args.get("description", "").strip()
    owner = args.get("owner", "").strip()

    if not name:
        return {"content": "", "error": "Vault name is required"}
    if not owner:
        return {"content": "", "error": "Owner (GitHub username) is required"}

    store = get_store()

    # Gather stats across all namespaces
    all_stats = store.stats()
    vault_stats = store.stats(namespace="vault")

    # Extract topics from vault embeddings
    topics = store.get_topics(namespace="vault", limit=15)

    # Also get topics from learned content
    for ns in all_stats.get("namespaces", []):
        if ns.startswith("learned/"):
            learned_topics = store.get_topics(namespace=ns, limit=5)
            topics.extend(t for t in learned_topics if t not in topics)

    # Get sample queries from observations if available
    sample_queries = args.get("sample_queries", [])

    # Detect installed plugins/jetpacks
    plugins = args.get("plugins", [])
    jetpacks = args.get("jetpacks", [])

    profile = {
        "format": "ludolph-vault-profile",
        "version": "1.0",
        "vault": {
            "name": name,
            "owner": owner,
            "description": description or f"{name} — a Ludolph-connected vault",
            "published_at": datetime.now(timezone.utc).isoformat(),
        },
        "knowledge": {
            "topics": topics[:20],
            "stats": {
                "total_chunks": all_stats.get("chunks", 0),
                "total_sources": all_stats.get("sources", 0),
                "vault_chunks": vault_stats.get("chunks", 0),
            },
            "namespaces": all_stats.get("namespaces", []),
        },
        "privacy": {
            "tier": args.get("tier", 1),
            "accepts_requests": args.get("accepts_requests", True),
        },
        "sample_queries": sample_queries,
        "plugins": plugins,
        "jetpacks": jetpacks,
    }

    formatted = json.dumps(profile, indent=2)
    return {
        "content": (
            f"Generated vault profile for '{name}':\n\n{formatted}\n\n"
            "Review this profile. If it looks good, use vault_publish_submit to publish it."
        )
    }


def _publish_submit(args: dict[str, Any]) -> dict:
    """Write profile locally and provide instructions for publishing."""
    profile_json = args.get("profile", "").strip()
    if not profile_json:
        # Try loading from the last generated profile
        if PROFILE_PATH.exists():
            profile_json = PROFILE_PATH.read_text()
        else:
            return {
                "content": "",
                "error": "No profile to publish. Run vault_publish first.",
            }

    # Validate JSON
    try:
        profile = json.loads(profile_json)
    except json.JSONDecodeError as e:
        return {"content": "", "error": f"Invalid profile JSON: {e}"}

    owner = profile.get("vault", {}).get("owner", "unknown")

    # Save locally
    PROFILE_PATH.parent.mkdir(parents=True, exist_ok=True)
    PROFILE_PATH.write_text(json.dumps(profile, indent=2))

    # Check if gh CLI is available
    try:
        subprocess.run(["gh", "--version"], capture_output=True, check=True)
        has_gh = True
    except (subprocess.CalledProcessError, FileNotFoundError):
        has_gh = False

    msg = f"Profile saved to {PROFILE_PATH}\n\n"

    if has_gh:
        msg += (
            "To publish to the registry:\n"
            f"1. Fork {REGISTRY_REPO} if you haven't already\n"
            f"2. Copy your profile: cp {PROFILE_PATH} vaults/{owner}.json\n"
            f"3. Create a PR: gh pr create --repo {REGISTRY_REPO}\n\n"
            "Or I can try to do this automatically — just say 'go ahead'."
        )
    else:
        msg += (
            "To publish to the registry:\n"
            f"1. Fork https://github.com/{REGISTRY_REPO}\n"
            f"2. Add your profile as vaults/{owner}.json\n"
            "3. Create a pull request\n\n"
            "Install the GitHub CLI (gh) for automated publishing."
        )

    return {"content": msg}


# --- Tool Definitions ---

TOOLS = [
    {
        "name": "vault_publish",
        "description": (
            "Generate a vault profile for publishing to the Ludolph community registry. "
            "Shows what your vault knows (topics, stats) without exposing content. "
            "Review the profile before submitting."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Display name for your vault",
                },
                "owner": {
                    "type": "string",
                    "description": "Your GitHub username",
                },
                "description": {
                    "type": "string",
                    "description": "One-paragraph description of what your vault knows about",
                },
                "tier": {
                    "type": "integer",
                    "description": "Privacy tier (1=metadata only, 2=structure, 3=full text). Default 1.",
                    "enum": [1, 2, 3],
                },
                "sample_queries": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Example queries visitors can try (optional)",
                },
                "accepts_requests": {
                    "type": "boolean",
                    "description": "Whether to accept learn requests from others (default true)",
                },
                "plugins": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Plugins you use (optional, for discoverability)",
                },
                "jetpacks": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Jetpacks you use (optional)",
                },
            },
            "required": ["name", "owner"],
        },
    },
    {
        "name": "vault_publish_submit",
        "description": (
            "Submit a vault profile to the community registry. "
            "Saves the profile locally and provides instructions for creating a PR."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "profile": {
                    "type": "string",
                    "description": "The profile JSON to publish (from vault_publish output)",
                },
            },
        },
    },
]

HANDLERS = {
    "vault_publish": _generate_profile,
    "vault_publish_submit": _publish_submit,
}
