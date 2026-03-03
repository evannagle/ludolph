"""MCP Registry - manages available and user-enabled MCPs."""

import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

import tomli_w


@dataclass
class McpDefinition:
    """Definition of an MCP server from the registry."""

    name: str
    description: str
    type: str  # "builtin" or "external"
    package: Optional[str] = None
    env_vars: list[str] = field(default_factory=list)
    test_tool: Optional[str] = None
    enabled: bool = True


class Registry:
    """Manages MCP definitions and user-specific configurations."""

    def __init__(self, registry_path: Path, users_path: Path):
        """
        Initialize the registry.

        Args:
            registry_path: Path to registry.toml file
            users_path: Path to users/ directory containing per-user configs
        """
        self.registry_path = registry_path
        self.users_path = users_path
        self._definitions: dict[str, McpDefinition] = {}
        self._load_registry()

    def _load_registry(self) -> None:
        """Load MCP definitions from registry.toml."""
        if not self.registry_path.exists():
            return

        with open(self.registry_path, "rb") as f:
            data = tomllib.load(f)

        for name, config in data.get("mcps", {}).items():
            self._definitions[name] = McpDefinition(
                name=name,
                description=config.get("description", ""),
                type=config.get("type", "external"),
                package=config.get("package"),
                env_vars=config.get("env_vars", []),
                test_tool=config.get("test_tool"),
                enabled=config.get("enabled", True),
            )

    def list_available(self) -> list[McpDefinition]:
        """
        List all available MCPs.

        Returns:
            List of MCP definitions from the registry
        """
        return list(self._definitions.values())

    def get_definition(self, name: str) -> Optional[McpDefinition]:
        """
        Get MCP definition by name.

        Args:
            name: The MCP name (e.g., "vault", "memory", "slack")

        Returns:
            McpDefinition if found, None otherwise
        """
        return self._definitions.get(name)

    def get_user_config(self, user_id: int) -> dict:
        """
        Get user-specific MCP configuration.

        Args:
            user_id: Telegram user ID

        Returns:
            User config dict with 'enabled' and 'credentials' sections
        """
        user_file = self.users_path / f"{user_id}.toml"
        if not user_file.exists():
            # Default config: vault and memory enabled
            return {"enabled": {"vault": True, "memory": True}, "credentials": {}}

        with open(user_file, "rb") as f:
            return tomllib.load(f)

    def get_user_enabled_mcps(self, user_id: int) -> list[str]:
        """
        Get list of MCPs enabled for a user.

        Args:
            user_id: Telegram user ID

        Returns:
            List of enabled MCP names
        """
        config = self.get_user_config(user_id)
        return [name for name, enabled in config.get("enabled", {}).items() if enabled]

    def is_external(self, name: str) -> bool:
        """
        Check if MCP is external (not builtin).

        Args:
            name: The MCP name

        Returns:
            True if MCP exists and is type "external"
        """
        defn = self._definitions.get(name)
        return defn is not None and defn.type == "external"

    def is_builtin(self, name: str) -> bool:
        """
        Check if MCP is builtin.

        Args:
            name: The MCP name

        Returns:
            True if MCP exists and is type "builtin"
        """
        defn = self._definitions.get(name)
        return defn is not None and defn.type == "builtin"

    def get_by_package(self, package: str) -> Optional[McpDefinition]:
        """
        Look up MCP definition by package name.

        Args:
            package: The npx/uvx package name (e.g., "mcp-server-slack")

        Returns:
            McpDefinition if found in registry, None otherwise
        """
        for defn in self._definitions.values():
            if defn.package == package:
                return defn
        return None

    def resolve_package(self, name_or_package: str) -> McpDefinition:
        """
        Resolve a name or package to an MCP definition.

        Works with:
        - Friendly names: "slack" -> looks up in registry
        - Package names: "mcp-server-slack" -> looks up or creates definition
        - Unknown packages: "mcp-server-custom" -> creates minimal definition

        Args:
            name_or_package: Either a friendly name or npx/uvx package name

        Returns:
            McpDefinition (from registry if known, auto-generated if not)
        """
        # Try friendly name first
        if name_or_package in self._definitions:
            return self._definitions[name_or_package]

        # Try package name lookup
        by_package = self.get_by_package(name_or_package)
        if by_package:
            return by_package

        # Unknown package - create minimal definition
        # Will be auto-discovered when spawned
        return McpDefinition(
            name=name_or_package,
            description=f"Custom MCP: {name_or_package}",
            type="external",
            package=name_or_package,
            env_vars=[],  # Will prompt if needed
            test_tool=None,
            enabled=True,
        )

    def get_user_custom_mcps(self, user_id: int) -> list[McpDefinition]:
        """
        Get user's custom MCPs (not in main registry).

        Args:
            user_id: Telegram user ID

        Returns:
            List of custom MCP definitions from user config
        """
        config = self.get_user_config(user_id)
        custom = []
        for name, mcp_config in config.get("custom_mcps", {}).items():
            custom.append(McpDefinition(
                name=name,
                description=mcp_config.get("description", f"Custom: {name}"),
                type="external",
                package=mcp_config.get("package", name),
                env_vars=mcp_config.get("env_vars", []),
                test_tool=mcp_config.get("test_tool"),
                enabled=mcp_config.get("enabled", True),
            ))
        return custom

    def _save_user_config(self, user_id: int, config: dict) -> None:
        """
        Save user configuration to TOML file.

        Args:
            user_id: Telegram user ID
            config: Configuration dict to save
        """
        user_file = self.users_path / f"{user_id}.toml"
        self.users_path.mkdir(parents=True, exist_ok=True)
        with open(user_file, "wb") as f:
            tomli_w.dump(config, f)

    def enable_mcp(
        self, user_id: int, name: str, credentials: Optional[dict[str, str]] = None
    ) -> bool:
        """
        Enable an MCP for a user.

        Args:
            user_id: Telegram user ID
            name: MCP name to enable
            credentials: Optional dict of env var name -> value

        Returns:
            True if MCP was enabled, False if MCP not found in registry
        """
        # Verify MCP exists in registry
        defn = self.get_definition(name)
        if defn is None:
            return False

        # Load existing config or create default
        config = self.get_user_config(user_id)

        # Ensure 'enabled' section exists
        if "enabled" not in config:
            config["enabled"] = {}

        # Enable the MCP
        config["enabled"][name] = True

        # Add credentials if provided
        if credentials:
            if "env" not in config:
                config["env"] = {}
            for key, value in credentials.items():
                config["env"][key] = value

        self._save_user_config(user_id, config)
        return True

    def disable_mcp(self, user_id: int, name: str) -> bool:
        """
        Disable an MCP for a user.

        Args:
            user_id: Telegram user ID
            name: MCP name to disable

        Returns:
            True if MCP was disabled, False if MCP not found in registry
        """
        # Verify MCP exists in registry
        defn = self.get_definition(name)
        if defn is None:
            return False

        # Load existing config or create default
        config = self.get_user_config(user_id)

        # Ensure 'enabled' section exists
        if "enabled" not in config:
            config["enabled"] = {}

        # Disable the MCP
        config["enabled"][name] = False

        self._save_user_config(user_id, config)
        return True
