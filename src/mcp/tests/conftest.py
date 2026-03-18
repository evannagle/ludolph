"""Pytest configuration for MCP tests.

Ensures consistent module imports by adding the MCP directory to sys.path
and creating module aliases so that both `mcp.security` and `security`
refer to the same module instance.
"""

import sys
from pathlib import Path

import pytest

# Add the MCP source directory to sys.path so imports work correctly
mcp_dir = Path(__file__).parent.parent
if str(mcp_dir) not in sys.path:
    sys.path.insert(0, str(mcp_dir))

# Also add parent for mcp.* imports to work
src_dir = mcp_dir.parent
if str(src_dir) not in sys.path:
    sys.path.insert(0, str(src_dir))

# Import the modules to ensure they're loaded with consistent names
import security
import tools
import tools.semantic
import llm
import server

# Create aliases so mcp.* refers to the same modules as the direct imports.
# This prevents the dual-module-instance bug where init_security()
# called on mcp.security doesn't affect code importing from security.
# It also ensures @patch decorators using mcp.* work correctly.
sys.modules["mcp"] = type(sys)("mcp")
sys.modules["mcp.security"] = security
sys.modules["mcp.tools"] = tools
sys.modules["mcp.tools.semantic"] = tools.semantic
sys.modules["mcp.llm"] = llm
sys.modules["mcp.server"] = server


@pytest.fixture(autouse=True)
def reset_semantic_model():
    """Reset the cached semantic model between tests.

    The semantic module caches the model in a global _model variable.
    Without resetting, tests that mock _get_model() to return None won't
    work if a previous test loaded the real model.
    """
    yield
    # Reset after each test
    tools.semantic._model = None
