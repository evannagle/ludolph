"""Tests for plugin manifest parsing."""

import tempfile
from pathlib import Path

import pytest

from plugins.manifest import (
    PluginManifest,
    PluginRuntime,
    PluginRuntimeType,
)


class TestPluginManifest:
    """Test PluginManifest parsing."""

    def test_parse_minimal_manifest(self):
        """Manifest with only required fields."""
        content = """
[plugin]
name = "lu-minimal"
version = "1.0.0"
"""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "lu-plugin.toml"
            path.write_text(content)

            manifest = PluginManifest.from_toml(path)

            assert manifest.name == "lu-minimal"
            assert manifest.version == "1.0.0"
            assert manifest.description == ""
            assert manifest.credentials == []
            assert manifest.tools == []
            assert manifest.schedules == []

    def test_parse_full_manifest(self):
        """Manifest with all fields."""
        content = """
[plugin]
name = "lu-full"
version = "2.1.0"
description = "A full plugin"
author = "test-author"
license = "MIT"
repository = "https://github.com/test/lu-full"
vault_integration = "Stores data in vault"

[plugin.runtime]
type = "external"
package = "lu-mcp-full"
runtime = "uvx"

[plugin.credentials]
API_KEY = { description = "API key", required = true }
SECRET = { description = "Optional secret", required = false, oauth_flow = "google" }

[[plugin.tools]]
name = "full_tool"
description = "A tool"
vault_output = true
output_path = "Output/{date}.md"

[[plugin.schedules]]
name = "daily_run"
cron = "0 9 * * *"
tool = "full_tool"
notify = true

[plugin.dependencies]
lu-version = ">=1.0.0"
"""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "lu-plugin.toml"
            path.write_text(content)

            manifest = PluginManifest.from_toml(path)

            assert manifest.name == "lu-full"
            assert manifest.version == "2.1.0"
            assert manifest.description == "A full plugin"
            assert manifest.author == "test-author"
            assert manifest.license == "MIT"
            assert manifest.repository == "https://github.com/test/lu-full"

            # Runtime
            assert manifest.runtime.type == PluginRuntimeType.EXTERNAL
            assert manifest.runtime.package == "lu-mcp-full"
            assert manifest.runtime.runtime == PluginRuntime.UVX

            # Credentials
            assert len(manifest.credentials) == 2
            api_key = next(c for c in manifest.credentials if c.name == "API_KEY")
            assert api_key.required is True
            assert api_key.oauth_flow is None
            secret = next(c for c in manifest.credentials if c.name == "SECRET")
            assert secret.required is False
            assert secret.oauth_flow == "google"

            # Tools
            assert len(manifest.tools) == 1
            assert manifest.tools[0].name == "full_tool"
            assert manifest.tools[0].vault_output is True
            assert manifest.tools[0].output_path == "Output/{date}.md"

            # Schedules
            assert len(manifest.schedules) == 1
            assert manifest.schedules[0].name == "daily_run"
            assert manifest.schedules[0].cron == "0 9 * * *"
            assert manifest.schedules[0].notify is True

            # Dependencies
            assert manifest.dependencies == {"lu-version": ">=1.0.0"}

    def test_parse_missing_name_raises(self):
        """Missing name field raises ValueError."""
        content = """
[plugin]
version = "1.0.0"
"""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "lu-plugin.toml"
            path.write_text(content)

            with pytest.raises(ValueError, match="missing required field: name"):
                PluginManifest.from_toml(path)

    def test_parse_missing_version_raises(self):
        """Missing version field raises ValueError."""
        content = """
[plugin]
name = "lu-test"
"""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "lu-plugin.toml"
            path.write_text(content)

            with pytest.raises(ValueError, match="missing required field: version"):
                PluginManifest.from_toml(path)

    def test_parse_invalid_runtime_type_raises(self):
        """Invalid runtime type raises ValueError."""
        content = """
[plugin]
name = "lu-test"
version = "1.0.0"

[plugin.runtime]
type = "invalid"
"""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "lu-plugin.toml"
            path.write_text(content)

            with pytest.raises(ValueError, match="Invalid runtime type"):
                PluginManifest.from_toml(path)

    def test_parse_file_not_found_raises(self):
        """Non-existent file raises FileNotFoundError."""
        with pytest.raises(FileNotFoundError):
            PluginManifest.from_toml(Path("/nonexistent/lu-plugin.toml"))

    def test_to_dict_roundtrip(self):
        """Manifest can be serialized to dict."""
        content = """
[plugin]
name = "lu-test"
version = "1.0.0"
description = "Test plugin"

[[plugin.tools]]
name = "test_tool"
description = "A test tool"
"""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "lu-plugin.toml"
            path.write_text(content)

            manifest = PluginManifest.from_toml(path)
            data = manifest.to_dict()

            assert data["name"] == "lu-test"
            assert data["version"] == "1.0.0"
            assert len(data["tools"]) == 1
            assert data["tools"][0]["name"] == "test_tool"

    def test_install_path_tracked(self):
        """Install path is tracked when loading manifest."""
        content = """
[plugin]
name = "lu-test"
version = "1.0.0"
"""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "lu-plugin.toml"
            path.write_text(content)

            manifest = PluginManifest.from_toml(path)

            assert manifest.install_path == Path(tmpdir)
