#!/bin/bash
# Package MCP server for release
#
# Usage:
#   ./scripts/package-mcp.sh              # Uses version from Cargo.toml
#   ./scripts/package-mcp.sh v0.5.0       # Uses specified version
#
# Creates: ludolph-mcp-{VERSION}.tar.gz containing the MCP server

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

echo "Packaging MCP server ${VERSION}..."

# Write VERSION file (strip the leading 'v' for the file content)
echo "${VERSION#v}" > src/mcp/VERSION

# Create tarball
tar -czf "$OUTFILE" -C src mcp/

echo "Created $OUTFILE"
echo
echo "Contents:"
tar -tzf "$OUTFILE" | head -20
