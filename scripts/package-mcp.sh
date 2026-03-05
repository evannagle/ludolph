#!/bin/bash
# Package MCP server for release
#
# Usage:
#   ./scripts/package-mcp.sh              # Uses version from Cargo.toml
#   ./scripts/package-mcp.sh v0.5.0       # Uses specified version
#
# Creates: ludolph-mcp-{VERSION}.tar.gz containing the MCP server
#
# Installation:
#   tar -xzf ludolph-mcp-*.tar.gz
#   ./install-mcp.sh

set -euo pipefail

cd "$(dirname "$0")/.."

# Get version: from argument, or extract from Cargo.toml
VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
    VERSION="v$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)"
fi

# Normalize: ensure starts with 'v'
[[ "$VERSION" != v* ]] && VERSION="v$VERSION"

OUTFILE="ludolph-mcp-${VERSION}.tar.gz"
STAGING_DIR=$(mktemp -d)

echo "Packaging MCP server ${VERSION}..."

# Write VERSION file (strip the leading 'v' for the file content)
echo "${VERSION#v}" > src/mcp/VERSION

# Copy files to staging
cp -r src/mcp "$STAGING_DIR/"
cp -r src/mcps "$STAGING_DIR/" 2>/dev/null || true
cp scripts/install-mcp.sh "$STAGING_DIR/"

# Create tarball
tar -czf "$OUTFILE" -C "$STAGING_DIR" .

# Cleanup
rm -rf "$STAGING_DIR"

echo "Created $OUTFILE"
echo
echo "Contents:"
tar -tzf "$OUTFILE" | head -25
echo
echo "Installation:"
echo "  tar -xzf $OUTFILE"
echo "  ./install-mcp.sh"
