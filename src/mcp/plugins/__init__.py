"""Lu Plugin System - extensibility layer for Ludolph.

This module provides the infrastructure for installing, managing, and running
Lu plugins. Plugins extend Lu's capabilities with external integrations while
maintaining the vault-first design principle.

Key concepts:
- Plugins are installed from git repos or local paths
- Each plugin has a lu-plugin.toml manifest
- Plugin tools are exposed via the MCP protocol
- Credentials are stored securely per-plugin
- Schedules allow automated plugin actions (e.g., daily digests)

Example usage:
    from plugins import PluginManager

    manager = PluginManager()
    manager.install("https://github.com/ludolph-community/lu-email")
    manager.setup("lu-email")  # Configure credentials
    manager.enable("lu-email")
"""

from .manager import PluginManager
from .manifest import (
    PluginCredential,
    PluginManifest,
    PluginRuntime,
    PluginRuntimeConfig,
    PluginRuntimeType,
    PluginSchedule,
    PluginTool,
)
from .storage import PluginStorage

__all__ = [
    "PluginManager",
    "PluginStorage",
    "PluginManifest",
    "PluginCredential",
    "PluginTool",
    "PluginSchedule",
    "PluginRuntimeConfig",
    "PluginRuntimeType",
    "PluginRuntime",
]
