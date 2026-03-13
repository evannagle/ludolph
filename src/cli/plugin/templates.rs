//! Embedded templates for plugin scaffolding.

pub const LU_PLUGIN_TOML: &str = r#"[plugin]
name = "{{name}}"
version = "0.1.0"
description = "{{description}}"
author = ""
license = "MIT"
repository = ""

[plugin.runtime]
type = "external"
package = "{{name}}"
runtime = "uvx"

# Uncomment and configure as needed:
# [plugin.credentials]
# API_KEY = { description = "API key for service", required = true }

# [[plugin.tools]]
# name = "tool_name"
# description = "What this tool does"
# vault_output = false

# [[plugin.schedules]]
# name = "daily_sync"
# cron = "0 8 * * *"
# tool = "tool_name"
# notify = true
"#;

pub const PYPROJECT_TOML: &str = r#"[project]
name = "{{name}}"
version = "0.1.0"
description = "{{description}}"
requires-python = ">=3.10"
dependencies = [
    "mcp>=1.0.0",
]

[project.optional-dependencies]
dev = [
    "pytest>=7.0.0",
    "pytest-asyncio>=0.21.0",
]

[tool.pytest.ini_options]
asyncio_mode = "auto"
testpaths = ["tests"]
"#;

pub const README_MD: &str = r"# {{name}}

{{description}}

## Installation

```bash
lu plugin install /path/to/{{name}}
lu plugin setup {{name}}
```

## Usage

Once installed, the tools are available through Lu's chat interface.

## Development

```bash
uv sync
uv run pytest
uv run mcp dev src/server.py
```

## License

MIT
";

pub const SRC_INIT_PY: &str = r#""""{{name}} - {{description}}"""

__version__ = "0.1.0"
"#;

pub const SERVER_PY: &str = r#"#!/usr/bin/env python3
"""{{name}} - {{description}}"""

from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import TextContent, Tool

server = Server("{{name}}")


@server.list_tools()
async def list_tools() -> list[Tool]:
    return [
        # Add your tools here
    ]


@server.call_tool()
async def call_tool(name: str, arguments: dict) -> list[TextContent]:
    # Handle your tools here
    raise ValueError(f"Unknown tool: {name}")


async def main():
    async with stdio_server() as (read_stream, write_stream):
        await server.run(
            read_stream,
            write_stream,
            server.create_initialization_options(),
        )


if __name__ == "__main__":
    import asyncio
    asyncio.run(main())
"#;

pub const TESTS_INIT_PY: &str = r#""""Tests for {{name}} plugin."""
"#;

pub const TEST_TOOLS_PY: &str = r#""""Tests for {{name}} plugin."""

import pytest

from src.server import list_tools


class TestListTools:
    @pytest.mark.asyncio
    async def test_list_tools_returns_list(self):
        tools = await list_tools()
        assert isinstance(tools, list)
"#;

/// CLAUDE.md template - AI development guidance for plugin authors
pub const CLAUDE_MD: &str = include_str!("../../../examples/plugins/lu-example/CLAUDE.md");
