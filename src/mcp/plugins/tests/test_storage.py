"""Tests for plugin storage."""

import tempfile
from pathlib import Path

import pytest

from plugins.storage import PluginStorage


class TestPluginStorage:
    """Test PluginStorage operations."""

    @pytest.fixture
    def storage(self):
        """Create a storage instance with temp directory."""
        with tempfile.TemporaryDirectory() as tmpdir:
            yield PluginStorage(base_path=Path(tmpdir))

    def test_init_creates_directories(self, storage):
        """Storage creates necessary directories on init."""
        assert storage.plugins_dir.exists()
        assert storage.base_path.exists()

    def test_get_plugin_path(self, storage):
        """Plugin path is under plugins directory."""
        path = storage.get_plugin_path("lu-test")
        assert path == storage.plugins_dir / "lu-test"

    def test_get_manifest_path(self, storage):
        """Manifest path is lu-plugin.toml in plugin directory."""
        path = storage.get_manifest_path("lu-test")
        assert path == storage.plugins_dir / "lu-test" / "lu-plugin.toml"

    def test_plugin_exists_false_when_not_installed(self, storage):
        """Plugin does not exist when not installed."""
        assert storage.plugin_exists("lu-nonexistent") is False

    def test_plugin_exists_true_when_manifest_exists(self, storage):
        """Plugin exists when manifest file exists."""
        plugin_dir = storage.get_plugin_path("lu-test")
        plugin_dir.mkdir(parents=True)
        (plugin_dir / "lu-plugin.toml").write_text("[plugin]\nname='lu-test'\nversion='1.0.0'")

        assert storage.plugin_exists("lu-test") is True

    def test_register_and_get_plugin(self, storage):
        """Register a plugin and retrieve its info."""
        storage.register_plugin(
            name="lu-test",
            version="1.0.0",
            install_path=storage.get_plugin_path("lu-test"),
            enabled=True,
        )

        info = storage.get_plugin_info("lu-test")
        assert info is not None
        assert info["version"] == "1.0.0"
        assert info["enabled"] is True

    def test_unregister_plugin(self, storage):
        """Unregister removes plugin from registry."""
        storage.register_plugin(
            name="lu-test",
            version="1.0.0",
            install_path=storage.get_plugin_path("lu-test"),
        )

        assert storage.unregister_plugin("lu-test") is True
        assert storage.get_plugin_info("lu-test") is None

    def test_unregister_nonexistent_returns_false(self, storage):
        """Unregistering nonexistent plugin returns False."""
        assert storage.unregister_plugin("lu-nonexistent") is False

    def test_list_plugins(self, storage):
        """List all registered plugins."""
        storage.register_plugin("lu-one", "1.0.0", storage.get_plugin_path("lu-one"))
        storage.register_plugin("lu-two", "2.0.0", storage.get_plugin_path("lu-two"))

        plugins = storage.list_plugins()
        names = [p["name"] for p in plugins]

        assert len(plugins) == 2
        assert "lu-one" in names
        assert "lu-two" in names

    def test_set_plugin_enabled(self, storage):
        """Enable/disable a plugin."""
        storage.register_plugin("lu-test", "1.0.0", storage.get_plugin_path("lu-test"), enabled=True)

        assert storage.is_plugin_enabled("lu-test") is True

        storage.set_plugin_enabled("lu-test", False)
        assert storage.is_plugin_enabled("lu-test") is False

        storage.set_plugin_enabled("lu-test", True)
        assert storage.is_plugin_enabled("lu-test") is True

    def test_set_enabled_nonexistent_returns_false(self, storage):
        """Setting enabled on nonexistent plugin returns False."""
        assert storage.set_plugin_enabled("lu-nonexistent", True) is False


class TestCredentialStorage:
    """Test credential storage operations."""

    @pytest.fixture
    def storage(self):
        """Create a storage instance with temp directory."""
        with tempfile.TemporaryDirectory() as tmpdir:
            yield PluginStorage(base_path=Path(tmpdir))

    def test_get_credentials_empty(self, storage):
        """Get credentials for plugin with no credentials returns empty dict."""
        creds = storage.get_credentials("lu-test")
        assert creds == {}

    def test_set_and_get_credential(self, storage):
        """Set a credential and retrieve it."""
        storage.set_credential("lu-test", "API_KEY", "secret123")

        creds = storage.get_credentials("lu-test")
        assert creds == {"API_KEY": "secret123"}

    def test_set_multiple_credentials(self, storage):
        """Set multiple credentials for a plugin."""
        storage.set_credential("lu-test", "API_KEY", "key1")
        storage.set_credential("lu-test", "SECRET", "secret2")

        creds = storage.get_credentials("lu-test")
        assert creds == {"API_KEY": "key1", "SECRET": "secret2"}

    def test_delete_credential(self, storage):
        """Delete a specific credential."""
        storage.set_credential("lu-test", "API_KEY", "key1")
        storage.set_credential("lu-test", "SECRET", "secret2")

        assert storage.delete_credential("lu-test", "API_KEY") is True

        creds = storage.get_credentials("lu-test")
        assert creds == {"SECRET": "secret2"}

    def test_delete_nonexistent_credential_returns_false(self, storage):
        """Deleting nonexistent credential returns False."""
        assert storage.delete_credential("lu-test", "NONEXISTENT") is False

    def test_delete_plugin_credentials(self, storage):
        """Delete all credentials for a plugin."""
        storage.set_credential("lu-test", "API_KEY", "key1")
        storage.set_credential("lu-test", "SECRET", "secret2")
        storage.set_credential("lu-other", "TOKEN", "token3")

        assert storage.delete_plugin_credentials("lu-test") is True

        assert storage.get_credentials("lu-test") == {}
        assert storage.get_credentials("lu-other") == {"TOKEN": "token3"}

    def test_list_configured_credentials(self, storage):
        """List which credentials are configured."""
        storage.set_credential("lu-test", "API_KEY", "key1")
        storage.set_credential("lu-test", "SECRET", "secret2")

        configured = storage.list_configured_credentials("lu-test")
        assert sorted(configured) == ["API_KEY", "SECRET"]

    def test_credentials_file_permissions(self, storage):
        """Credentials file has restricted permissions."""
        storage.set_credential("lu-test", "SECRET", "sensitive")

        # File should exist and be readable only by owner
        assert storage.credentials_file.exists()
        mode = storage.credentials_file.stat().st_mode & 0o777
        assert mode == 0o600
