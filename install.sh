#!/usr/bin/env bash
set -euo pipefail

# Ludolph installer
# Usage: curl -sSL https://raw.githubusercontent.com/evannagle/ludolph/main/install.sh | bash

REPO="evannagle/ludolph"
INSTALL_DIR="$HOME/.ludolph/bin"
DATA_DIR="$HOME/ludolph"

echo "Installing Ludolph..."
echo

# Detect OS and architecture
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
    x86_64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
    linux) TARGET="${ARCH}-unknown-linux-gnu" ;;
    darwin) TARGET="${ARCH}-apple-darwin" ;;
    *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

# Get latest release
LATEST=$(curl -sSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
if [ -z "$LATEST" ]; then
    echo "Could not determine latest version. Using main branch."
    LATEST="main"
fi

echo "Detected: $OS $ARCH"
echo "Installing version: $LATEST"
echo

# Create directories
mkdir -p "$INSTALL_DIR"
mkdir -p "$DATA_DIR/vault"
mkdir -p "$DATA_DIR/logs"

# Download binary
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST/lu-$TARGET"
echo "Downloading from: $DOWNLOAD_URL"

if ! curl -sSL "$DOWNLOAD_URL" -o "$INSTALL_DIR/lu"; then
    echo "Download failed. Release may not exist yet."
    echo "Build from source: cargo install --git https://github.com/$REPO"
    exit 1
fi

chmod +x "$INSTALL_DIR/lu"

# Add to PATH
SHELL_RC=""
if [ -f "$HOME/.zshrc" ]; then
    SHELL_RC="$HOME/.zshrc"
elif [ -f "$HOME/.bashrc" ]; then
    SHELL_RC="$HOME/.bashrc"
fi

if [ -n "$SHELL_RC" ]; then
    if ! grep -q '.ludolph/bin' "$SHELL_RC" 2>/dev/null; then
        echo 'export PATH="$HOME/.ludolph/bin:$PATH"' >> "$SHELL_RC"
        echo "Added to PATH in $SHELL_RC"
    fi
fi

export PATH="$INSTALL_DIR:$PATH"

# Interactive setup
echo
echo "=== Configuration ==="
echo

if [ ! -f "$DATA_DIR/config.toml" ]; then
    echo "Let's set up your config."
    echo
    echo "1. Telegram Bot Token"
    echo "   Create a bot: https://t.me/BotFather"
    echo "   Send /newbot and follow the prompts"
    echo
    read -rp "Enter your Telegram bot token: " TELEGRAM_TOKEN
    echo

    echo "2. Claude API Key"
    echo "   Get one: https://console.anthropic.com/settings/keys"
    echo
    read -rp "Enter your Anthropic API key: " CLAUDE_KEY
    echo

    cat > "$DATA_DIR/config.toml" << EOF
[telegram]
bot_token = "$TELEGRAM_TOKEN"

[claude]
api_key = "$CLAUDE_KEY"
model = "claude-sonnet-4-20250514"

[vault]
path = "$DATA_DIR/vault"
EOF

    echo "Config written to $DATA_DIR/config.toml"
fi

echo
echo "=== Installation Complete ==="
echo
echo "Next steps:"
echo "  1. Sync your Obsidian vault to: $DATA_DIR/vault/"
echo "  2. Start Ludolph: lu"
echo "  3. Message your Telegram bot!"
echo
echo "Commands:"
echo "  lu          Start the bot"
echo "  lu status   Check status"
echo "  lu config   Edit configuration"
echo
