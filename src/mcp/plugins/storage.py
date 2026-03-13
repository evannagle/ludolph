"""Plugin storage - file paths and state management."""

import json
import logging
from pathlib import Path
from typing import Optional

import tomllib
import tomli_w

logger = logging.getLogger(__name__)


class PluginStorage:
    """
    Manages plugin storage paths and state files.

    Directory structure:
        ~/.ludolph/
            plugins/                    # Installed plugins
                lu-email/              # Plugin directory (cloned from git)
                    lu-plugin.toml     # Plugin manifest
                    ...
                lu-calendar/
                    lu-plugin.toml
                    ...
            plugins.toml               # Installed plugin registry
            credentials.json           # Plugin credentials (encrypted on macOS)

    Plugin registry format (plugins.toml):
        [plugins.lu-email]
        version = "1.0.0"
        enabled = true
        install_path = "~/.ludolph/plugins/lu-email"
        installed_at = "2026-03-12T15:00:00Z"

        [plugins.lu-calendar]
        version = "2.1.0"
        enabled = false
        install_path = "~/.ludolph/plugins/lu-calendar"
        installed_at = "2026-03-11T10:30:00Z"
    """

    def __init__(self, base_path: Optional[Path] = None):
        """
        Initialize plugin storage.

        Args:
            base_path: Base path for ludolph data. Defaults to ~/.ludolph
        """
        self.base_path = base_path or (Path.home() / ".ludolph")
        self.plugins_dir = self.base_path / "plugins"
        self.registry_file = self.base_path / "plugins.toml"
        self.credentials_file = self.base_path / "credentials.json"

        # Ensure directories exist
        self.plugins_dir.mkdir(parents=True, exist_ok=True)

    def get_plugin_path(self, name: str) -> Path:
        """
        Get the installation path for a plugin.

        Args:
            name: Plugin name

        Returns:
            Path to plugin directory
        """
        return self.plugins_dir / name

    def get_manifest_path(self, name: str) -> Path:
        """
        Get the manifest path for a plugin.

        Args:
            name: Plugin name

        Returns:
            Path to lu-plugin.toml
        """
        return self.get_plugin_path(name) / "lu-plugin.toml"

    def plugin_exists(self, name: str) -> bool:
        """
        Check if a plugin is installed.

        Args:
            name: Plugin name

        Returns:
            True if plugin directory and manifest exist
        """
        manifest_path = self.get_manifest_path(name)
        return manifest_path.exists()

    def load_registry(self) -> dict:
        """
        Load the installed plugins registry.

        Returns:
            Registry dict with plugin entries
        """
        if not self.registry_file.exists():
            return {"plugins": {}}

        try:
            with open(self.registry_file, "rb") as f:
                return tomllib.load(f)
        except Exception as e:
            logger.warning(f"Failed to load plugin registry: {e}")
            return {"plugins": {}}

    def save_registry(self, registry: dict) -> None:
        """
        Save the installed plugins registry.

        Args:
            registry: Registry dict to save
        """
        with open(self.registry_file, "wb") as f:
            tomli_w.dump(registry, f)

    def register_plugin(
        self,
        name: str,
        version: str,
        install_path: Path,
        enabled: bool = True,
    ) -> None:
        """
        Register an installed plugin.

        Args:
            name: Plugin name
            version: Plugin version
            install_path: Path where plugin is installed
            enabled: Whether plugin is enabled
        """
        import datetime

        registry = self.load_registry()

        if "plugins" not in registry:
            registry["plugins"] = {}

        registry["plugins"][name] = {
            "version": version,
            "enabled": enabled,
            "install_path": str(install_path),
            "installed_at": datetime.datetime.now(datetime.UTC).isoformat(),
        }

        self.save_registry(registry)

    def unregister_plugin(self, name: str) -> bool:
        """
        Remove a plugin from the registry.

        Args:
            name: Plugin name

        Returns:
            True if plugin was removed, False if not found
        """
        registry = self.load_registry()

        if name not in registry.get("plugins", {}):
            return False

        del registry["plugins"][name]
        self.save_registry(registry)
        return True

    def get_plugin_info(self, name: str) -> Optional[dict]:
        """
        Get registry info for a plugin.

        Args:
            name: Plugin name

        Returns:
            Plugin registry entry or None if not found
        """
        registry = self.load_registry()
        return registry.get("plugins", {}).get(name)

    def list_plugins(self) -> list[dict]:
        """
        List all installed plugins.

        Returns:
            List of plugin info dicts with name included
        """
        registry = self.load_registry()
        plugins = []
        for name, info in registry.get("plugins", {}).items():
            plugins.append({"name": name, **info})
        return plugins

    def set_plugin_enabled(self, name: str, enabled: bool) -> bool:
        """
        Enable or disable a plugin.

        Args:
            name: Plugin name
            enabled: Whether to enable

        Returns:
            True if plugin was updated, False if not found
        """
        registry = self.load_registry()

        if name not in registry.get("plugins", {}):
            return False

        registry["plugins"][name]["enabled"] = enabled
        self.save_registry(registry)
        return True

    def is_plugin_enabled(self, name: str) -> bool:
        """
        Check if a plugin is enabled.

        Args:
            name: Plugin name

        Returns:
            True if plugin exists and is enabled
        """
        info = self.get_plugin_info(name)
        return info is not None and info.get("enabled", False)

    # -------------------------------------------------------------------------
    # Credential Management
    # -------------------------------------------------------------------------

    def _load_credentials(self) -> dict:
        """Load credentials file."""
        if not self.credentials_file.exists():
            return {}

        try:
            with open(self.credentials_file) as f:
                return json.load(f)
        except Exception as e:
            logger.warning(f"Failed to load credentials: {e}")
            return {}

    def _save_credentials(self, credentials: dict) -> None:
        """Save credentials file."""
        # Set restrictive permissions on credentials file
        with open(self.credentials_file, "w") as f:
            json.dump(credentials, f, indent=2)

        # Make file readable only by owner
        self.credentials_file.chmod(0o600)

    def get_credentials(self, plugin_name: str) -> dict[str, str]:
        """
        Get credentials for a plugin.

        Args:
            plugin_name: Plugin name

        Returns:
            Dict of credential name -> value
        """
        credentials = self._load_credentials()
        return credentials.get(plugin_name, {})

    def set_credential(self, plugin_name: str, key: str, value: str) -> None:
        """
        Set a credential for a plugin.

        Args:
            plugin_name: Plugin name
            key: Credential key
            value: Credential value
        """
        credentials = self._load_credentials()

        if plugin_name not in credentials:
            credentials[plugin_name] = {}

        credentials[plugin_name][key] = value
        self._save_credentials(credentials)

    def delete_credential(self, plugin_name: str, key: str) -> bool:
        """
        Delete a credential for a plugin.

        Args:
            plugin_name: Plugin name
            key: Credential key

        Returns:
            True if credential was deleted, False if not found
        """
        credentials = self._load_credentials()

        if plugin_name not in credentials:
            return False

        if key not in credentials[plugin_name]:
            return False

        del credentials[plugin_name][key]
        self._save_credentials(credentials)
        return True

    def delete_plugin_credentials(self, plugin_name: str) -> bool:
        """
        Delete all credentials for a plugin.

        Args:
            plugin_name: Plugin name

        Returns:
            True if credentials were deleted, False if none found
        """
        credentials = self._load_credentials()

        if plugin_name not in credentials:
            return False

        del credentials[plugin_name]
        self._save_credentials(credentials)
        return True

    def list_configured_credentials(self, plugin_name: str) -> list[str]:
        """
        List which credentials are configured for a plugin.

        Args:
            plugin_name: Plugin name

        Returns:
            List of configured credential keys
        """
        credentials = self.get_credentials(plugin_name)
        return list(credentials.keys())
