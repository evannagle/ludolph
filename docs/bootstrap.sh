#!/usr/bin/env bash
# Ludolph Bootstrap - Downloads lu binary and runs setup
# Usage: curl -sSL https://ludolph.dev/install | bash
set -euo pipefail

REPO="evannagle/ludolph"
LOCAL_DIR="${LUDOLPH_DIR:-$HOME/.ludolph}"
VERSION="${LUDOLPH_VERSION:-}"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
DIM='\033[0;90m'
NC='\033[0m'

ok() { printf "  ${GREEN}[ok]${NC} %s\n" "$1"; }
err() { printf "  ${RED}[!!]${NC} %s\n" "$1"; }
info() { printf "  ${DIM}%s${NC}\n" "$1"; }

# Detect platform
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
    x86_64) ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *) err "Unsupported architecture: $ARCH"; exit 1 ;;
esac

case "$OS" in
    darwin) TARGET="${ARCH}-apple-darwin" ;;
    linux) TARGET="${ARCH}-unknown-linux-gnu" ;;
    *) err "Unsupported OS: $OS"; exit 1 ;;
esac

echo
echo "Ludolph Bootstrap"
echo

# Fetch version if not specified
if [ -z "$VERSION" ]; then
    info "Fetching latest version..."
    VERSION=$(curl -sSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null | grep '"tag_name"' | cut -d'"' -f4 || true)
    [ -z "$VERSION" ] && { err "Could not fetch version"; exit 1; }
fi
ok "Version: $VERSION"

# Download binary
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/lu-$TARGET"
BIN_DIR="$LOCAL_DIR/bin"
mkdir -p "$BIN_DIR"

info "Downloading lu binary..."
if ! curl -fsSL "$DOWNLOAD_URL" -o "$BIN_DIR/lu"; then
    err "Failed to download from $DOWNLOAD_URL"
    exit 1
fi
chmod +x "$BIN_DIR/lu"
ok "Installed to $BIN_DIR/lu"

# Add to PATH for current session
export PATH="$BIN_DIR:$PATH"

# Add to shell profile if not already there
for profile in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
    if [ -f "$profile" ] && ! grep -q '.ludolph/bin' "$profile" 2>/dev/null; then
        echo 'export PATH="$HOME/.ludolph/bin:$PATH"' >> "$profile"
        ok "Added to $profile"
        break
    fi
done

echo
info "Running lu setup..."
echo

# Run setup
exec "$BIN_DIR/lu" setup
