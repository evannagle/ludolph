"""Tests for plugin manager."""

import tempfile
from pathlib import Path

import pytest

from plugins.manager import PluginInstallError, PluginManager, PluginNotFoundError
from plugins.storage import PluginStorage


class TestPluginManager:
    """Test PluginManager operations."""

    @pytest.fixture
    def manager(self):
        """Create a manager with temp storage."""
        with tempfile.TemporaryDirectory() as tmpdir:
            storage = PluginStorage(base_path=Path(tmpdir))
            yield PluginManager(storage=storage)

    @pytest.fixture
    def sample_plugin(self, manager):
        """Create a sample plugin directory."""
        plugin_dir = manager.storage.plugins_dir / "lu-sample"
        plugin_dir.mkdir(parents=True)

        manifest = """
[plugin]
name = "lu-sample"
version = "1.0.0"
description = "A sample plugin"

[plugin.credentials]
API_KEY = { description = "API key", required = true }
OPTIONAL = { description = "Optional", required = false }

[[plugin.tools]]
name = "sample_tool"
description = "A sample tool"
"""
        (plugin_dir / "lu-plugin.toml").write_text(manifest)

        # Register in storage
        manager.storage.register_plugin(
            name="lu-sample",
            version="1.0.0",
            install_path=plugin_dir,
            enabled=True,
        )

        return plugin_dir

    def test_install_from_path(self, manager):
        """Install plugin from local path."""
        with tempfile.TemporaryDirectory() as tmpdir:
            # Create source plugin
            source = Path(tmpdir) / "source-plugin"
            source.mkdir()
            (source / "lu-plugin.toml").write_text("""
[plugin]
name = "lu-local"
version = "1.0.0"
description = "Local plugin"
""")

            manifest = manager.install(str(source))

            assert manifest.name == "lu-local"
            assert manifest.version == "1.0.0"
            assert manager.storage.plugin_exists("lu-local")

    def test_install_missing_manifest_raises(self, manager):
        """Install from path without manifest raises error."""
        with tempfile.TemporaryDirectory() as tmpdir:
            source = Path(tmpdir) / "no-manifest"
            source.mkdir()

            with pytest.raises(PluginInstallError, match="No lu-plugin.toml"):
                manager.install(str(source))

    def test_install_already_installed_raises(self, manager, sample_plugin):
        """Installing already-installed plugin raises error."""
        with tempfile.TemporaryDirectory() as tmpdir:
            source = Path(tmpdir) / "duplicate"
            source.mkdir()
            (source / "lu-plugin.toml").write_text("""
[plugin]
name = "lu-sample"
version = "2.0.0"
""")

            with pytest.raises(PluginInstallError, match="already installed"):
                manager.install(str(source))

    def test_remove_plugin(self, manager, sample_plugin):
        """Remove an installed plugin."""
        assert manager.storage.plugin_exists("lu-sample")

        result = manager.remove("lu-sample")

        assert result is True
        assert not manager.storage.plugin_exists("lu-sample")
        assert not sample_plugin.exists()

    def test_remove_nonexistent_returns_false(self, manager):
        """Removing nonexistent plugin returns False."""
        assert manager.remove("lu-nonexistent") is False

    def test_get_plugin(self, manager, sample_plugin):
        """Get plugin manifest."""
        manifest = manager.get("lu-sample")

        assert manifest.name == "lu-sample"
        assert manifest.version == "1.0.0"

    def test_get_nonexistent_raises(self, manager):
        """Getting nonexistent plugin raises error."""
        with pytest.raises(PluginNotFoundError):
            manager.get("lu-nonexistent")

    def test_list_plugins(self, manager, sample_plugin):
        """List all installed plugins."""
        plugins = manager.list()

        assert len(plugins) == 1
        assert plugins[0].name == "lu-sample"

    def test_enable_disable(self, manager, sample_plugin):
        """Enable and disable plugins."""
        assert manager.is_enabled("lu-sample") is True

        manager.disable("lu-sample")
        assert manager.is_enabled("lu-sample") is False

        manager.enable("lu-sample")
        assert manager.is_enabled("lu-sample") is True

    def test_check_plugin(self, manager, sample_plugin):
        """Health check returns plugin status."""
        # Set one credential
        manager.storage.set_credential("lu-sample", "API_KEY", "test-key")

        result = manager.check("lu-sample")

        assert result["name"] == "lu-sample"
        assert result["version"] == "1.0.0"
        assert result["enabled"] is True
        assert result["credentials"]["API_KEY"] is True
        assert result["credentials"]["OPTIONAL"] is False
        assert result["credentials_complete"] is True  # Only required creds
        assert "sample_tool" in result["tools"]

    def test_needs_setup(self, manager, sample_plugin):
        """Check if plugin needs credential setup."""
        # No credentials set
        assert manager.needs_setup("lu-sample") is True

        # Set required credential
        manager.storage.set_credential("lu-sample", "API_KEY", "test-key")
        assert manager.needs_setup("lu-sample") is False

    def test_check_nonexistent_raises(self, manager):
        """Health check on nonexistent plugin raises error."""
        with pytest.raises(PluginNotFoundError):
            manager.check("lu-nonexistent")
