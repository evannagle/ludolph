#!/bin/bash
# Install Ludolph MCP server
#
# Usage:
#   ./install-mcp.sh                    # Install with all features
#   ./install-mcp.sh --no-semantic      # Skip semantic search (smaller install)
#
# Prerequisites:
#   - Python 3.11+ with uv (pip install uv)
#   - macOS (for launchd service)
#
# This script:
#   1. Creates ~/.ludolph/mcp directory structure
#   2. Copies MCP server files
#   3. Creates Python venv with dependencies
#   4. Sets up launchd service
#   5. Starts the service

set -euo pipefail

# Configuration
MCP_DIR="$HOME/.ludolph/mcp"
LAUNCHD_PLIST="$HOME/Library/LaunchAgents/dev.ludolph.mcp.plist"
SERVICE_NAME="dev.ludolph.mcp"
DEFAULT_PORT=8202

# Parse arguments
INSTALL_SEMANTIC=true
for arg in "$@"; do
    case $arg in
        --no-semantic)
            INSTALL_SEMANTIC=false
            shift
            ;;
    esac
done

echo "Installing Ludolph MCP Server..."
echo

# Check prerequisites
if ! command -v uv &> /dev/null; then
    echo "Error: 'uv' not found. Install with: pip install uv"
    exit 1
fi

if ! command -v python3 &> /dev/null; then
    echo "Error: python3 not found"
    exit 1
fi

PYTHON_VERSION=$(python3 -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')")
if [[ "$(echo "$PYTHON_VERSION < 3.11" | bc)" == "1" ]]; then
    echo "Error: Python 3.11+ required, found $PYTHON_VERSION"
    exit 1
fi

# Determine script directory (where mcp/ folder is)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [[ -d "$SCRIPT_DIR/mcp" ]]; then
    # Extracted from tarball (install-mcp.sh alongside mcp/)
    SOURCE_DIR="$SCRIPT_DIR"
elif [[ -d "$SCRIPT_DIR/../src/mcp" ]]; then
    # Running from repo (scripts/install-mcp.sh, mcp is in src/)
    SOURCE_DIR="$SCRIPT_DIR/../src"
elif [[ -d "$SCRIPT_DIR/../mcp" ]]; then
    # Alternative tarball layout
    SOURCE_DIR="$SCRIPT_DIR/.."
else
    echo "Error: Cannot find mcp/ directory relative to install script"
    echo "Looking in: $SCRIPT_DIR/mcp, $SCRIPT_DIR/../src/mcp, $SCRIPT_DIR/../mcp"
    exit 1
fi

# Create directory structure
echo "[1/5] Creating directory structure..."
mkdir -p "$MCP_DIR/tools"
mkdir -p "$HOME/.ludolph/bin"

# Copy MCP files
echo "[2/5] Copying MCP server files..."
cp -r "$SOURCE_DIR/mcp/"* "$MCP_DIR/"
if [[ -d "$SOURCE_DIR/mcps" ]]; then
    mkdir -p "$HOME/.ludolph/mcps"
    cp -r "$SOURCE_DIR/mcps/"* "$HOME/.ludolph/mcps/"
fi

# Create/update venv and install dependencies
echo "[3/5] Setting up Python environment..."
cd "$MCP_DIR"

if [[ ! -d ".venv" ]]; then
    uv venv
fi

# Install base dependencies
uv pip install flask litellm

# Install semantic search dependencies if requested
if [[ "$INSTALL_SEMANTIC" == "true" ]]; then
    echo "     Installing semantic search dependencies (this may take a while)..."
    uv pip install sentence-transformers numpy
fi

# Install dev dependencies for testing
uv pip install pytest

# Generate auth token if not exists
TOKEN_FILE="$HOME/.ludolph/mcp_token"
if [[ ! -f "$TOKEN_FILE" ]]; then
    echo "[4/5] Generating auth token..."
    openssl rand -hex 32 > "$TOKEN_FILE"
    chmod 600 "$TOKEN_FILE"
else
    echo "[4/5] Auth token exists, keeping..."
fi
AUTH_TOKEN=$(cat "$TOKEN_FILE")

# Prompt for vault path if not set
VAULT_PATH="${VAULT_PATH:-}"
if [[ -z "$VAULT_PATH" ]]; then
    echo
    read -p "Enter path to Obsidian vault: " VAULT_PATH
fi

if [[ ! -d "$VAULT_PATH" ]]; then
    echo "Warning: Vault path '$VAULT_PATH' does not exist"
fi

# Get and validate Anthropic API key
ANTHROPIC_API_KEY="${ANTHROPIC_API_KEY:-}"
if [[ -z "$ANTHROPIC_API_KEY" ]]; then
    # Check if already in existing plist
    if [[ -f "$LAUNCHD_PLIST" ]]; then
        ANTHROPIC_API_KEY=$(plutil -extract EnvironmentVariables.ANTHROPIC_API_KEY raw "$LAUNCHD_PLIST" 2>/dev/null || echo "")
    fi
fi

# Validate API key (loop until valid or user skips)
validate_api_key() {
    local key="$1"
    if [[ -z "$key" ]]; then
        return 1
    fi
    # Test with a minimal API call
    local response
    response=$(curl -s -w "\n%{http_code}" https://api.anthropic.com/v1/messages \
        -H "x-api-key: $key" \
        -H "anthropic-version: 2023-06-01" \
        -H "content-type: application/json" \
        -d '{"model":"claude-sonnet-4-20250514","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}' 2>/dev/null)
    local status_code
    status_code=$(echo "$response" | tail -1)
    [[ "$status_code" == "200" ]]
}

while true; do
    if [[ -z "$ANTHROPIC_API_KEY" ]]; then
        echo
        echo "Get your API key from: https://console.anthropic.com/account/keys"
        read -p "Enter Anthropic API key (sk-ant-...): " ANTHROPIC_API_KEY
    fi

    if [[ -z "$ANTHROPIC_API_KEY" ]]; then
        echo "Warning: No API key provided. LLM features will not work."
        break
    fi

    echo "     Validating API key..."
    if validate_api_key "$ANTHROPIC_API_KEY"; then
        echo "     API key is valid."
        break
    else
        echo
        echo "Error: API key is invalid or expired."
        echo "       Get a new key from: https://console.anthropic.com/account/keys"
        echo
        read -p "Enter a different key (or press Enter to skip): " ANTHROPIC_API_KEY
        if [[ -z "$ANTHROPIC_API_KEY" ]]; then
            echo "Warning: Skipping API key. LLM features will not work until configured."
            break
        fi
    fi
done

# Create launchd plist
echo "[5/5] Setting up launchd service..."

# Use venv Python
VENV_PYTHON="$MCP_DIR/.venv/bin/python"

cat > "$LAUNCHD_PLIST" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>$SERVICE_NAME</string>
    <key>ProgramArguments</key>
    <array>
        <string>$VENV_PYTHON</string>
        <string>-m</string>
        <string>mcp.server</string>
    </array>
    <key>WorkingDirectory</key>
    <string>$HOME/.ludolph</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>VAULT_PATH</key>
        <string>$VAULT_PATH</string>
        <key>AUTH_TOKEN</key>
        <string>$AUTH_TOKEN</string>
        <key>PORT</key>
        <string>$DEFAULT_PORT</string>
        <key>ANTHROPIC_API_KEY</key>
        <string>$ANTHROPIC_API_KEY</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>$MCP_DIR/server.log</string>
    <key>StandardErrorPath</key>
    <string>$MCP_DIR/server.log</string>
</dict>
</plist>
EOF

# Load/reload service
launchctl unload "$LAUNCHD_PLIST" 2>/dev/null || true
launchctl load "$LAUNCHD_PLIST"

# Wait and verify
sleep 2
if curl -s "http://localhost:$DEFAULT_PORT/health" -H "Authorization: Bearer $AUTH_TOKEN" | grep -q "ok"; then
    echo
    echo "Ludolph MCP Server installed successfully!"
    echo
    echo "  Service:  $SERVICE_NAME"
    echo "  Endpoint: http://localhost:$DEFAULT_PORT"
    echo "  Token:    $TOKEN_FILE"
    echo "  Logs:     $MCP_DIR/server.log"
    echo
    echo "Commands:"
    echo "  View logs:     tail -f $MCP_DIR/server.log"
    echo "  Restart:       launchctl kickstart -k gui/\$(id -u)/$SERVICE_NAME"
    echo "  Stop:          launchctl unload $LAUNCHD_PLIST"
else
    echo
    echo "Warning: Service may not have started correctly."
    echo "Check logs: tail -f $MCP_DIR/server.log"
fi
