"""Plugin Manager - install, remove, and manage plugins."""

from __future__ import annotations

import logging
import shutil
import subprocess
from pathlib import Path
from typing import Optional

from .manifest import PluginManifest
from .storage import PluginStorage

logger = logging.getLogger(__name__)


class PluginError(Exception):
    """Base exception for plugin operations."""

    pass


class PluginNotFoundError(PluginError):
    """Plugin not found."""

    pass


class PluginInstallError(PluginError):
    """Plugin installation failed."""

    pass


class PluginManager:
    """
    Manages plugin installation, removal, and configuration.

    Supports installing plugins from:
    - Git URLs: https://github.com/ludolph-community/lu-email
    - Local paths: /path/to/local/plugin
    - Registry names: lu-email (looks up in community registry)
    """

    def __init__(self, storage: Optional[PluginStorage] = None):
        """
        Initialize plugin manager.

        Args:
            storage: Plugin storage instance. Defaults to ~/.ludolph
        """
        self.storage = storage or PluginStorage()

    def install(self, source: str) -> PluginManifest:
        """
        Install a plugin from source.

        Args:
            source: Git URL, local path, or registry name

        Returns:
            Installed plugin manifest

        Raises:
            PluginInstallError: If installation fails
        """
        # Determine source type
        if source.startswith(("http://", "https://", "git@")):
            return self._install_from_git(source)
        elif Path(source).exists():
            return self._install_from_path(Path(source))
        else:
            # Try registry lookup
            return self._install_from_registry(source)

    def _install_from_git(self, url: str) -> PluginManifest:
        """Install plugin from git repository."""
        # Extract plugin name from URL
        # https://github.com/ludolph-community/lu-email -> lu-email
        name = url.rstrip("/").rsplit("/", 1)[-1]
        if name.endswith(".git"):
            name = name[:-4]

        install_path = self.storage.get_plugin_path(name)

        # Check if already installed
        if install_path.exists():
            raise PluginInstallError(
                f"Plugin {name} already installed. Remove first with: lu plugin remove {name}"
            )

        # Clone repository
        logger.info(f"Cloning {url} to {install_path}")
        try:
            result = subprocess.run(
                ["git", "clone", "--depth", "1", url, str(install_path)],
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode != 0:
                raise PluginInstallError(f"Git clone failed: {result.stderr}")
        except FileNotFoundError:
            raise PluginInstallError("Git is not installed")

        # Validate manifest
        return self._finalize_install(name, install_path)

    def _install_from_path(self, path: Path) -> PluginManifest:
        """Install plugin from local path."""
        manifest_path = path / "lu-plugin.toml"
        if not manifest_path.exists():
            raise PluginInstallError(f"No lu-plugin.toml found in {path}")

        # Parse manifest to get name
        manifest = PluginManifest.from_toml(manifest_path)
        name = manifest.name

        install_path = self.storage.get_plugin_path(name)

        # Check if already installed
        if install_path.exists():
            raise PluginInstallError(
                f"Plugin {name} already installed. Remove first with: lu plugin remove {name}"
            )

        # Copy directory
        logger.info(f"Copying {path} to {install_path}")
        shutil.copytree(path, install_path)

        return self._finalize_install(name, install_path)

    def _install_from_registry(self, name: str) -> PluginManifest:
        """Install plugin from community registry."""
        # For now, assume github.com/ludolph-community/<name>
        url = f"https://github.com/ludolph-community/{name}"
        try:
            return self._install_from_git(url)
        except PluginInstallError:
            raise PluginInstallError(
                f"Plugin {name} not found in registry. "
                f"Try a full URL: lu plugin install https://github.com/..."
            )

    def _finalize_install(self, name: str, install_path: Path) -> PluginManifest:
        """Validate and register installed plugin."""
        manifest_path = install_path / "lu-plugin.toml"

        if not manifest_path.exists():
            # Clean up failed install
            shutil.rmtree(install_path, ignore_errors=True)
            raise PluginInstallError(f"No lu-plugin.toml found in {name}")

        try:
            manifest = PluginManifest.from_toml(manifest_path)
        except ValueError as e:
            # Clean up failed install
            shutil.rmtree(install_path, ignore_errors=True)
            raise PluginInstallError(f"Invalid manifest: {e}")

        # Register in storage
        self.storage.register_plugin(
            name=manifest.name,
            version=manifest.version,
            install_path=install_path,
            enabled=True,
        )

        logger.info(f"Installed plugin {manifest.name} v{manifest.version}")
        return manifest

    def remove(self, name: str) -> bool:
        """
        Remove an installed plugin.

        Args:
            name: Plugin name

        Returns:
            True if removed, False if not found
        """
        if not self.storage.plugin_exists(name):
            return False

        # Remove from registry
        self.storage.unregister_plugin(name)

        # Remove credentials
        self.storage.delete_plugin_credentials(name)

        # Remove files
        plugin_path = self.storage.get_plugin_path(name)
        if plugin_path.exists():
            shutil.rmtree(plugin_path)

        logger.info(f"Removed plugin {name}")
        return True

    def get(self, name: str) -> PluginManifest:
        """
        Get a plugin manifest.

        Args:
            name: Plugin name

        Returns:
            Plugin manifest

        Raises:
            PluginNotFoundError: If plugin not installed
        """
        manifest_path = self.storage.get_manifest_path(name)
        if not manifest_path.exists():
            raise PluginNotFoundError(f"Plugin not found: {name}")

        manifest = PluginManifest.from_toml(manifest_path)

        # Add enabled state from registry
        info = self.storage.get_plugin_info(name)
        if info:
            manifest.enabled = info.get("enabled", True)

        return manifest

    def list(self) -> list[PluginManifest]:
        """
        List all installed plugins.

        Returns:
            List of plugin manifests
        """
        plugins = []
        for info in self.storage.list_plugins():
            name = info["name"]
            try:
                manifest = self.get(name)
                plugins.append(manifest)
            except Exception as e:
                logger.warning(f"Failed to load plugin {name}: {e}")
        return plugins

    def enable(self, name: str) -> bool:
        """
        Enable a plugin.

        Args:
            name: Plugin name

        Returns:
            True if enabled, False if not found
        """
        return self.storage.set_plugin_enabled(name, True)

    def disable(self, name: str) -> bool:
        """
        Disable a plugin.

        Args:
            name: Plugin name

        Returns:
            True if disabled, False if not found
        """
        return self.storage.set_plugin_enabled(name, False)

    def is_enabled(self, name: str) -> bool:
        """
        Check if a plugin is enabled.

        Args:
            name: Plugin name

        Returns:
            True if enabled
        """
        return self.storage.is_plugin_enabled(name)

    def update(self, name: str) -> Optional[PluginManifest]:
        """
        Update a plugin to latest version.

        Args:
            name: Plugin name

        Returns:
            Updated manifest, or None if no update available
        """
        plugin_path = self.storage.get_plugin_path(name)
        if not plugin_path.exists():
            raise PluginNotFoundError(f"Plugin not found: {name}")

        # Check if it's a git repository
        git_dir = plugin_path / ".git"
        if not git_dir.exists():
            logger.info(f"Plugin {name} is not a git repository, cannot update")
            return None

        # Get current version
        old_manifest = self.get(name)
        old_version = old_manifest.version

        # Fetch and pull
        logger.info(f"Updating plugin {name}...")
        result = subprocess.run(
            ["git", "-C", str(plugin_path), "pull", "--ff-only"],
            capture_output=True,
            text=True,
            check=False,
        )

        if result.returncode != 0:
            logger.warning(f"Git pull failed: {result.stderr}")
            return None

        # Re-parse manifest
        new_manifest = self.get(name)

        if new_manifest.version != old_version:
            # Update registry
            self.storage.register_plugin(
                name=name,
                version=new_manifest.version,
                install_path=plugin_path,
                enabled=old_manifest.enabled,
            )
            logger.info(f"Updated {name} from {old_version} to {new_manifest.version}")
            return new_manifest

        logger.info(f"Plugin {name} is already up to date ({old_version})")
        return None

    def update_all(self) -> list[PluginManifest]:
        """
        Update all installed plugins.

        Returns:
            List of updated plugin manifests
        """
        updated = []
        for manifest in self.list():
            try:
                result = self.update(manifest.name)
                if result:
                    updated.append(result)
            except Exception as e:
                logger.warning(f"Failed to update {manifest.name}: {e}")
        return updated

    def check(self, name: str) -> dict:
        """
        Run health check on a plugin.

        Args:
            name: Plugin name

        Returns:
            Health check results

        Raises:
            PluginNotFoundError: If plugin not found
        """
        manifest = self.get(name)

        # Check credentials
        configured_creds = self.storage.list_configured_credentials(name)
        required_creds = [c.name for c in manifest.credentials if c.required]
        missing_creds = [c for c in required_creds if c not in configured_creds]

        credentials_status = {}
        for cred in manifest.credentials:
            credentials_status[cred.name] = cred.name in configured_creds

        return {
            "name": manifest.name,
            "version": manifest.version,
            "enabled": manifest.enabled,
            "credentials": credentials_status,
            "credentials_complete": len(missing_creds) == 0,
            "missing_credentials": missing_creds,
            "tools": [t.name for t in manifest.tools],
            "schedules": [s.name for s in manifest.schedules],
        }

    def needs_setup(self, name: str) -> bool:
        """
        Check if a plugin needs credential setup.

        Args:
            name: Plugin name

        Returns:
            True if any required credentials are missing
        """
        manifest = self.get(name)
        configured_creds = self.storage.list_configured_credentials(name)
        required_creds = [c.name for c in manifest.credentials if c.required]
        return any(c not in configured_creds for c in required_creds)
