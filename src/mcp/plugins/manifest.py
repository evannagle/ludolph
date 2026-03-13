"""Plugin manifest parsing - lu-plugin.toml format."""

import tomllib
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Optional


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
    # Internal tracking
    install_path: Optional[Path] = None
    enabled: bool = True

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

        manifest = cls(
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

        # Track where the manifest was loaded from
        manifest.install_path = path.parent

        return manifest

    def to_dict(self) -> dict:
        """Convert manifest to dictionary for JSON serialization."""
        return {
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "author": self.author,
            "license": self.license,
            "repository": self.repository,
            "vault_integration": self.vault_integration,
            "runtime": {
                "type": self.runtime.type.value,
                "package": self.runtime.package,
                "runtime": self.runtime.runtime.value,
            },
            "credentials": [
                {
                    "name": c.name,
                    "description": c.description,
                    "required": c.required,
                    "oauth_flow": c.oauth_flow,
                }
                for c in self.credentials
            ],
            "tools": [
                {
                    "name": t.name,
                    "description": t.description,
                    "vault_output": t.vault_output,
                    "output_path": t.output_path,
                }
                for t in self.tools
            ],
            "schedules": [
                {
                    "name": s.name,
                    "cron": s.cron,
                    "tool": s.tool,
                    "notify": s.notify,
                }
                for s in self.schedules
            ],
            "dependencies": self.dependencies,
            "enabled": self.enabled,
        }
