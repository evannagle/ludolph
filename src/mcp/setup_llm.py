#!/usr/bin/env python3
"""
Setup LLM credentials for the Ludolph MCP server.

Guides users through configuring their Claude Code subscription
or other LLM provider credentials.
"""

import os
import subprocess
import sys
from pathlib import Path

ENV_FILE = Path(__file__).parent / ".env"


def load_env():
    """Load existing .env file if it exists."""
    env = {}
    if ENV_FILE.exists():
        for line in ENV_FILE.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, value = line.split("=", 1)
                env[key.strip()] = value.strip().strip('"').strip("'")
    return env


def save_env(env: dict):
    """Save environment variables to .env file."""
    lines = []
    for key, value in sorted(env.items()):
        # Quote values with spaces
        if " " in value or not value:
            value = f'"{value}"'
        lines.append(f"{key}={value}")
    ENV_FILE.write_text("\n".join(lines) + "\n")
    print(f"Saved to {ENV_FILE}")


def check_claude_cli():
    """Check if claude CLI is available."""
    try:
        result = subprocess.run(
            ["claude", "--version"],
            capture_output=True,
            text=True,
        )
        return result.returncode == 0
    except FileNotFoundError:
        return False


def setup_claude_code_token():
    """Run claude setup-token to get OAuth token."""
    print("\n[Claude Code Setup]")
    print("This will open a browser to authenticate with your Claude subscription.")
    print("The generated token will be saved for the MCP server to use.\n")

    response = input("Continue? [Y/n] ").strip().lower()
    if response and response != "y":
        print("Skipped.")
        return None

    try:
        result = subprocess.run(
            ["claude", "setup-token"],
            capture_output=True,
            text=True,
        )
        if result.returncode == 0:
            # The token is printed to stdout
            token = result.stdout.strip()
            if token.startswith("sk-ant-"):
                print("Token generated successfully.")
                return token
            else:
                # Token might be in a different format or need manual entry
                print("\nToken generated. Please enter it below:")
                token = input("Token: ").strip()
                if token:
                    return token
        else:
            print(f"Error: {result.stderr}")
            return None
    except Exception as e:
        print(f"Error running claude setup-token: {e}")
        return None


def setup_api_key():
    """Prompt for manual API key entry."""
    print("\n[API Key Setup]")
    print("Enter your Anthropic API key (starts with sk-ant-api03-...):")
    print("Get one at: https://console.anthropic.com/settings/keys\n")

    key = input("API Key: ").strip()
    if key and key.startswith("sk-ant-"):
        return key
    elif key:
        print("Warning: Key doesn't look like an Anthropic key, but saving anyway.")
        return key
    return None


def setup_vault_path():
    """Prompt for vault path."""
    print("\n[Vault Path]")
    default = os.path.expanduser("~/vault")
    path = input(f"Path to your vault [{default}]: ").strip()
    if not path:
        path = default
    path = os.path.expanduser(path)

    if not os.path.isdir(path):
        create = input(f"Directory doesn't exist. Create it? [Y/n] ").strip().lower()
        if not create or create == "y":
            os.makedirs(path, exist_ok=True)
            print(f"Created {path}")
        else:
            return None

    return path


def setup_auth_token():
    """Generate or prompt for MCP auth token."""
    print("\n[MCP Auth Token]")
    print("This token protects your MCP server from unauthorized access.")

    import secrets

    default = secrets.token_urlsafe(32)
    token = input(f"Auth token [{default[:16]}...]: ").strip()
    if not token:
        token = default
        print(f"Generated: {token[:16]}...")

    return token


def main():
    """Run the setup wizard."""
    print("=" * 60)
    print("Ludolph MCP Server Setup")
    print("=" * 60)

    env = load_env()

    # Check for existing config
    if env.get("ANTHROPIC_API_KEY"):
        print(f"\nExisting API key found: {env['ANTHROPIC_API_KEY'][:20]}...")
        response = input("Reconfigure? [y/N] ").strip().lower()
        if response != "y":
            print("Keeping existing configuration.")
            return

    # LLM Provider setup
    print("\n[LLM Provider]")
    print("1. Claude Code subscription (recommended - uses your Max plan)")
    print("2. Anthropic API key (prepaid credits)")
    print("3. Skip (configure later)")

    choice = input("\nChoice [1]: ").strip() or "1"

    if choice == "1":
        if not check_claude_cli():
            print("\nError: 'claude' CLI not found.")
            print("Install it first: npm install -g @anthropic-ai/claude-code")
            print("\nFalling back to API key option...")
            choice = "2"
        else:
            token = setup_claude_code_token()
            if token:
                env["ANTHROPIC_API_KEY"] = token

    if choice == "2":
        key = setup_api_key()
        if key:
            env["ANTHROPIC_API_KEY"] = key

    # Vault path
    if not env.get("VAULT_PATH"):
        path = setup_vault_path()
        if path:
            env["VAULT_PATH"] = path

    # Auth token
    if not env.get("AUTH_TOKEN"):
        token = setup_auth_token()
        if token:
            env["AUTH_TOKEN"] = token

    # Save
    if env:
        save_env(env)
        print("\n" + "=" * 60)
        print("Setup complete!")
        print("=" * 60)
        print("\nTo start the server:")
        print(f"  cd {Path(__file__).parent}")
        print("  source .venv/bin/activate")
        print("  python -m flask --app server run --port 8200")
        print("\nOr with the .env file loaded:")
        print("  set -a && source .env && set +a && python -m flask --app server run --port 8200")
    else:
        print("\nNo configuration saved.")


if __name__ == "__main__":
    main()
