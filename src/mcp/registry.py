"""MCP Registry - manages available and user-enabled MCPs."""

import tomllib
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Optional

import tomli_w


class PluginRuntimeType(Enum):
    """Plugin runtime type."""

    BUILTIN = "builtin"
    EXTERNAL = "external"
    WEBHOOK = "webhook"


class PluginRuntime(Enum):
    """Plugin runtime environment."""

    NPX = "npx"
    UVX = "uvx"
    PYTHON = "python"


@dataclass
class PluginCredential:
    """A credential required by a plugin."""

    name: str
    description: str
    required: bool = True
    oauth_flow: Optional[str] = None  # e.g., "google", "microsoft"


@dataclass
class PluginTool:
    """A tool provided by a plugin."""

    name: str
    description: str
    vault_output: bool = False
    output_path: Optional[str] = None  # e.g., "Inbox/email-summaries/{date}.md"


@dataclass
class PluginSchedule:
    """A scheduled task for a plugin."""

    name: str
    cron: str  # cron expression
    tool: str  # tool name to invoke
    notify: bool = False


@dataclass
class PluginRuntimeConfig:
    """Runtime configuration for a plugin."""

    type: PluginRuntimeType = PluginRuntimeType.EXTERNAL
    package: Optional[str] = None
    runtime: PluginRuntime = PluginRuntime.NPX


@dataclass
class PluginManifest:
    """
    Plugin manifest representing lu-plugin.toml format.

    Example manifest:
        [plugin]
        name = "lu-email"
        version = "1.0.0"
        description = "Gmail integration with vault-first email management"
        author = "ludolph-community"
        license = "MIT"
        repository = "https://github.com/ludolph-community/lu-email"

        [plugin.runtime]
        type = "external"
        package = "lu-mcp-email"
        runtime = "npx"

        [plugin.credentials]
        GOOGLE_CLIENT_ID = { description = "OAuth client ID", required = true }

        [[plugin.tools]]
        name = "email_summarize_inbox"
        description = "Summarize unread emails and create vault note"
        vault_output = true
        output_path = "Inbox/email-summaries/{date}.md"

        [[plugin.schedules]]
        name = "inbox_digest"
        cron = "0 8 * * *"
        tool = "email_summarize_inbox"
        notify = true

        [plugin.dependencies]
        lu-version = ">=0.9.0"
    """

    name: str
    version: str
    description: str
    author: str = ""
    license: str = ""
    repository: str = ""
    vault_integration: str = ""
    runtime: PluginRuntimeConfig = field(default_factory=PluginRuntimeConfig)
    credentials: list[PluginCredential] = field(default_factory=list)
    tools: list[PluginTool] = field(default_factory=list)
    schedules: list[PluginSchedule] = field(default_factory=list)
    dependencies: dict[str, str] = field(default_factory=dict)

    @classmethod
    def from_toml(cls, path: Path) -> "PluginManifest":
        """
        Parse a plugin manifest from a TOML file.

        Args:
            path: Path to lu-plugin.toml file

        Returns:
            PluginManifest instance

        Raises:
            FileNotFoundError: If manifest file doesn't exist
            ValueError: If manifest is invalid or missing required fields
        """
        if not path.exists():
            raise FileNotFoundError(f"Plugin manifest not found: {path}")

        with open(path, "rb") as f:
            data = tomllib.load(f)

        plugin = data.get("plugin", {})

        # Required fields
        name = plugin.get("name")
        version = plugin.get("version")
        description = plugin.get("description", "")

        if not name:
            raise ValueError("Plugin manifest missing required field: name")
        if not version:
            raise ValueError("Plugin manifest missing required field: version")

        # Parse runtime config
        runtime_data = plugin.get("runtime", {})
        runtime_type_str = runtime_data.get("type", "external")
        try:
            runtime_type = PluginRuntimeType(runtime_type_str)
        except ValueError:
            raise ValueError(f"Invalid runtime type: {runtime_type_str}")

        runtime_env_str = runtime_data.get("runtime", "npx")
        try:
            runtime_env = PluginRuntime(runtime_env_str)
        except ValueError:
            raise ValueError(f"Invalid runtime: {runtime_env_str}")

        runtime = PluginRuntimeConfig(
            type=runtime_type,
            package=runtime_data.get("package"),
            runtime=runtime_env,
        )

        # Parse credentials
        credentials = []
        for cred_name, cred_config in plugin.get("credentials", {}).items():
            if isinstance(cred_config, dict):
                credentials.append(PluginCredential(
                    name=cred_name,
                    description=cred_config.get("description", ""),
                    required=cred_config.get("required", True),
                    oauth_flow=cred_config.get("oauth_flow"),
                ))
            else:
                # Simple string value means description only
                credentials.append(PluginCredential(
                    name=cred_name,
                    description=str(cred_config),
                ))

        # Parse tools
        tools = []
        for tool_data in plugin.get("tools", []):
            tools.append(PluginTool(
                name=tool_data.get("name", ""),
                description=tool_data.get("description", ""),
                vault_output=tool_data.get("vault_output", False),
                output_path=tool_data.get("output_path"),
            ))

        # Parse schedules
        schedules = []
        for sched_data in plugin.get("schedules", []):
            schedules.append(PluginSchedule(
                name=sched_data.get("name", ""),
                cron=sched_data.get("cron", ""),
                tool=sched_data.get("tool", ""),
                notify=sched_data.get("notify", False),
            ))

        return cls(
            name=name,
            version=version,
            description=description,
            author=plugin.get("author", ""),
            license=plugin.get("license", ""),
            repository=plugin.get("repository", ""),
            vault_integration=plugin.get("vault_integration", ""),
            runtime=runtime,
            credentials=credentials,
            tools=tools,
            schedules=schedules,
            dependencies=plugin.get("dependencies", {}),
        )

    def to_mcp_definition(self) -> "McpDefinition":
        """Convert plugin manifest to MCP definition for registry integration."""
        return McpDefinition(
            name=self.name,
            description=self.description,
            type=self.runtime.type.value,
            package=self.runtime.package,
            env_vars=[c.name for c in self.credentials],
            test_tool=self.tools[0].name if self.tools else None,
            enabled=True,
        )


@dataclass
class McpDefinition:
    """Definition of an MCP server from the registry."""

    name: str
    description: str
    type: str  # "builtin" or "external"
    package: Optional[str] = None
    runtime: str = "npx"  # "npx" or "uvx"
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
                runtime=config.get("runtime", "npx"),
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
