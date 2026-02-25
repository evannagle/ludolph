# Ludolph MCP Server

A general-purpose filesystem access server that gives Lu read/write access to any folder. Works with Obsidian vaults, code repositories, or any directory structure.

## Architecture

```
Pi (Lu) ──HTTP──> server.py (Mac) ──filesystem──> Your folder
                      │
                      ├── /health      Health check
                      ├── /tools       List available tools
                      └── /tools/call  Execute a tool
```

The MCP server runs on your Mac and exposes your folder over HTTP. The Pi connects to it to read and write files on your behalf. All requests require a Bearer token for authentication.

## Quick Start

```bash
# Start the server
VAULT_PATH=/path/to/folder AUTH_TOKEN=secret PORT=8200 python -m mcp.server

# Or run directly
cd src/mcp
VAULT_PATH=~/vault AUTH_TOKEN=secret python server.py
```

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `VAULT_PATH` | Yes | `~/vault` | Root directory for file operations |
| `AUTH_TOKEN` | Yes | (none) | Bearer token for authentication |
| `PORT` | No | `8200` | Server port |

## Endpoints

### `GET /`

Server info (no auth required).

```bash
curl http://localhost:8200/
```

```json
{
  "name": "Ludolph MCP Server",
  "version": "0.5.0",
  "status": "running"
}
```

### `GET /health`

Health check with vault info.

```bash
curl -H "Authorization: Bearer $TOKEN" http://localhost:8200/health
```

```json
{
  "status": "ok",
  "vault": "/Users/you/vault",
  "git_repo": true
}
```

### `GET /tools`

List all available tools.

```bash
curl -H "Authorization: Bearer $TOKEN" http://localhost:8200/tools
```

Returns an array of tool definitions with name, description, and input schema.

### `POST /tools/call`

Execute a tool.

```bash
curl -X POST http://localhost:8200/tools/call \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "read_file", "arguments": {"path": "notes/todo.md"}}'
```

```json
{
  "content": "# Todo\n- Buy milk\n- Fix bug",
  "error": null
}
```

## Tools

### File Operations

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents |
| `write_file` | Create or replace a file |
| `append_file` | Append content to end of file |
| `delete_file` | Delete a file |
| `move_file` | Move or rename a file |

### Directory Operations

| Tool | Description |
|------|-------------|
| `list_directory` | List directory contents |
| `create_directory` | Create a directory (including parents) |

### Search

| Tool | Description |
|------|-------------|
| `search` | Simple text search across filenames and content |
| `search_advanced` | Regex and glob pattern search |

### Metadata

| Tool | Description |
|------|-------------|
| `file_info` | Get file metadata (size, dates, permissions) |

## Tool Reference

### read_file

Read the contents of a file.

```json
{
  "name": "read_file",
  "arguments": {
    "path": "notes/daily/2024-01-15.md"
  }
}
```

### write_file

Create or replace a file. Creates parent directories if needed.

```json
{
  "name": "write_file",
  "arguments": {
    "path": "notes/new-note.md",
    "content": "# New Note\n\nContent here."
  }
}
```

### append_file

Append content to the end of a file. Adds a newline before content if the file doesn't end with one. Creates the file if it doesn't exist.

```json
{
  "name": "append_file",
  "arguments": {
    "path": "notes/log.md",
    "content": "- New entry"
  }
}
```

### delete_file

Delete a file.

```json
{
  "name": "delete_file",
  "arguments": {
    "path": "notes/old-note.md"
  }
}
```

### move_file

Move or rename a file. Creates destination directories if needed.

```json
{
  "name": "move_file",
  "arguments": {
    "source": "inbox/note.md",
    "destination": "archive/2024/note.md"
  }
}
```

### list_directory

List directory contents. Hidden files (starting with `.`) are excluded.

```json
{
  "name": "list_directory",
  "arguments": {
    "path": "notes"
  }
}
```

Returns:
```
dir: daily
dir: projects
file: README.md
file: todo.md
```

### create_directory

Create a directory, including parent directories.

```json
{
  "name": "create_directory",
  "arguments": {
    "path": "notes/projects/2024"
  }
}
```

### search

Simple text search. Searches both filenames and file content.

```json
{
  "name": "search",
  "arguments": {
    "query": "meeting notes",
    "path": "notes",
    "context_length": 50
  }
}
```

| Parameter | Required | Default | Description |
|-----------|----------|---------|-------------|
| `query` | Yes | | Search term |
| `path` | No | root | Subdirectory to search |
| `context_length` | No | 50 | Characters of context around matches |

### search_advanced

Advanced search with regex and glob patterns.

```json
{
  "name": "search_advanced",
  "arguments": {
    "pattern": "TODO:\\s+.*",
    "glob": "*.md",
    "path": "notes",
    "content_only": true
  }
}
```

| Parameter | Required | Default | Description |
|-----------|----------|---------|-------------|
| `pattern` | Yes | | Regex pattern |
| `path` | No | root | Subdirectory to search |
| `glob` | No | `*` | Glob pattern to filter files |
| `content_only` | No | false | Skip filename matches |

### file_info

Get file metadata.

```json
{
  "name": "file_info",
  "arguments": {
    "path": "notes/todo.md"
  }
}
```

Returns:
```
path: notes/todo.md
type: file
size: 1234 bytes
created: 2024-01-15T10:30:00
modified: 2024-01-16T14:22:00
permissions: 644
```

## Security

### Path Validation

All paths are validated before any operation:

- Paths containing `..` are rejected
- Paths are resolved and verified to be within the vault root
- Symlinks pointing outside the vault are rejected

```python
# These are rejected:
../etc/passwd
notes/../../../etc/passwd
..
```

### Authentication

All endpoints except `/` require a Bearer token:

```bash
curl -H "Authorization: Bearer your-token-here" http://localhost:8200/health
```

Requests without a valid token receive a 401 Unauthorized response.

### Git Awareness

The server detects if the root directory is a git repository:

- `is_git_repo()` returns true if `.git` exists
- `is_git_ignored(path)` checks if a file is in `.gitignore`
- `file_info` includes git status in its output

## Testing

Run the unit tests:

```bash
cd src/mcp
python -m unittest tests.test_tools -v
```

Test with curl:

```bash
# Start server
VAULT_PATH=/tmp/test AUTH_TOKEN=secret python server.py &

# Create test file
echo "Hello" > /tmp/test/hello.md

# Test read
curl -X POST http://localhost:8200/tools/call \
  -H "Authorization: Bearer secret" \
  -H "Content-Type: application/json" \
  -d '{"name": "read_file", "arguments": {"path": "hello.md"}}'
```

## Development

### Project Structure

```
src/mcp/
├── __init__.py      # Package exports
├── server.py        # Flask app and routes
├── tools.py         # Tool definitions and implementations
├── security.py      # Path validation and auth
├── tests/
│   ├── __init__.py
│   └── test_tools.py
└── README.md        # This file
```

### Adding a New Tool

1. Add the tool definition to `TOOLS` in `tools.py`:

```python
{
    "name": "my_tool",
    "description": "What it does",
    "input_schema": {
        "type": "object",
        "properties": {
            "param": {"type": "string", "description": "..."}
        },
        "required": ["param"]
    }
}
```

2. Implement the handler function:

```python
def _my_tool(args: dict) -> dict:
    """Implement the tool."""
    path = safe_path(args.get("path", ""))
    if not path:
        return {"content": "", "error": "Invalid path"}

    # Do work...

    return {"content": "result", "error": None}
```

3. Register in `call_tool()`:

```python
handlers = {
    # ...existing handlers...
    "my_tool": _my_tool,
}
```

4. Add tests in `tests/test_tools.py`

5. Run tests: `python -m unittest tests.test_tools -v`

## Deployment

The installer (`docs/install`) embeds this server and configures it to run automatically via launchd on macOS. Configuration is stored at:

- `~/.ludolph/mcp/server.py` — The server script
- `~/.ludolph/mcp_token` — The auth token
- `~/Library/LaunchAgents/dev.ludolph.mcp.plist` — launchd config

To manually manage the server:

```bash
# Stop
launchctl stop dev.ludolph.mcp

# Start
launchctl start dev.ludolph.mcp

# View logs
tail -f ~/.ludolph/mcp/server.log
```
