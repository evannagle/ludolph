#!/usr/bin/env python3
"""
Ludolph MCP Server - Claude Code native MCP integration.

Exposes channel communication tools via the MCP protocol for direct
Claude Code integration (no HTTP/curl needed).

This server communicates with the Pi's HTTP API for channel messaging.
The Pi is the single source of truth for all channel messages.

Usage:
    # From installed location
    python ~/.ludolph/mcp/mcp_server.py

    # Or add to Claude Code's ~/.mcp.json:
    {
        "mcpServers": {
            "ludolph": {
                "type": "stdio",
                "command": "python3",
                "args": ["~/.ludolph/mcp/mcp_server.py"],
                "env": {
                    "PI_HOST": "your-pi-hostname",
                    "PI_CHANNEL_PORT": "8202",
                    "CHANNEL_AUTH_TOKEN": "your-token"
                }
            }
        }
    }
"""

import os
import time

import requests

from mcp.server.fastmcp import FastMCP

# Pi connection configuration
PI_HOST = os.environ.get("PI_HOST", "localhost")
PI_CHANNEL_PORT = os.environ.get("PI_CHANNEL_PORT", "8202")
CHANNEL_AUTH_TOKEN = os.environ.get("CHANNEL_AUTH_TOKEN", "")

# Initialize MCP server
mcp = FastMCP("ludolph")


def _get_pi_url(path: str) -> str:
    """Build URL for Pi's channel API."""
    return f"http://{PI_HOST}:{PI_CHANNEL_PORT}{path}"


def _get_headers() -> dict:
    """Get headers for Pi API requests."""
    return {"Authorization": f"Bearer {CHANNEL_AUTH_TOKEN}"}


@mcp.tool()
def channel_send(
    content: str,
    reply_to: int | None = None,
    wait_for_response: bool = True,
    timeout: int = 60,
) -> str:
    """
    Send a message to Lu and optionally wait for response.

    Args:
        content: The message to send to Lu
        reply_to: Optional message ID this is replying to
        wait_for_response: Wait for Lu's response (default: true, max 60s)
        timeout: Max seconds to wait for response (default: 60)

    Returns:
        Lu's response if wait_for_response is True, otherwise confirmation
    """
    content = content.strip()
    if not content:
        return "Error: Message content is required"

    try:
        # Send to Pi
        resp = requests.post(
            _get_pi_url("/channel/send"),
            headers=_get_headers(),
            json={"from": "claude_code", "content": content, "reply_to": reply_to},
            timeout=10,
        )
        resp.raise_for_status()
        data = resp.json()
        sent_id = data["id"]

        if not wait_for_response:
            return f"Message sent (ID: {sent_id}). Lu will respond shortly."

        # Poll for Lu's response
        start_time = time.time()
        while time.time() - start_time < timeout:
            time.sleep(2)
            hist_resp = requests.get(
                _get_pi_url("/channel/history"),
                headers=_get_headers(),
                params={"limit": 10},
                timeout=10,
            )
            hist_resp.raise_for_status()
            messages = hist_resp.json()["messages"]
            for m in reversed(messages):
                if m["sender"] == "lu" and m.get("reply_to") == sent_id:
                    return f"Lu's response:\n\n{m['content']}"

        return f"Message sent (ID: {sent_id}) but no response within {timeout}s."

    except requests.exceptions.ConnectionError:
        return f"Error: Pi unreachable at {PI_HOST}:{PI_CHANNEL_PORT}. Is Ludolph running?"
    except Exception as e:
        return f"Error: {e}"


@mcp.tool()
def channel_history(limit: int = 20) -> str:
    """
    Get recent channel conversation history between Claude Code and Lu.

    Args:
        limit: Maximum messages to return (default 20)

    Returns:
        Formatted channel history
    """
    try:
        resp = requests.get(
            _get_pi_url("/channel/history"),
            headers=_get_headers(),
            params={"limit": limit},
            timeout=10,
        )
        resp.raise_for_status()
        messages = resp.json()["messages"]

        if not messages:
            return "No channel messages yet."

        lines = ["Channel History:\n"]
        for msg in messages:
            ts = msg["timestamp"][11:19]
            direction = "<- lu" if msg["sender"] == "lu" else "-> lu"
            line = f"[{ts}] {msg['sender']} {direction}: {msg['content'][:100]}"
            if len(msg["content"]) > 100:
                line += "..."
            lines.append(line)

        return "\n".join(lines)

    except requests.exceptions.ConnectionError:
        return f"Error: Pi unreachable at {PI_HOST}:{PI_CHANNEL_PORT}"
    except Exception as e:
        return f"Error: {e}"


def main():
    """Run the MCP server."""
    mcp.run()


if __name__ == "__main__":
    main()
