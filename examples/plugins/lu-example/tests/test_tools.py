"""Tests for lu-example plugin tools."""

import pytest

from src.server import call_tool, list_tools


class TestListTools:
    """Test tool listing."""

    @pytest.mark.asyncio
    async def test_list_tools_returns_two_tools(self):
        """Plugin should provide exactly 2 tools."""
        tools = await list_tools()
        assert len(tools) == 2

    @pytest.mark.asyncio
    async def test_list_tools_has_greet(self):
        """Plugin should provide example_greet tool."""
        tools = await list_tools()
        names = [t.name for t in tools]
        assert "example_greet" in names

    @pytest.mark.asyncio
    async def test_list_tools_has_summarize(self):
        """Plugin should provide example_summarize tool."""
        tools = await list_tools()
        names = [t.name for t in tools]
        assert "example_summarize" in names


class TestGreetTool:
    """Test the example_greet tool."""

    @pytest.mark.asyncio
    async def test_greet_casual(self):
        """Casual greeting should include name."""
        result = await call_tool("example_greet", {"name": "Alice"})
        assert len(result) == 1
        assert "Alice" in result[0].text
        assert "Hey" in result[0].text

    @pytest.mark.asyncio
    async def test_greet_formal(self):
        """Formal greeting should be polite."""
        result = await call_tool("example_greet", {"name": "Bob", "style": "formal"})
        assert "Bob" in result[0].text
        assert "pleasure" in result[0].text.lower()

    @pytest.mark.asyncio
    async def test_greet_pirate(self):
        """Pirate greeting should say ahoy."""
        result = await call_tool("example_greet", {"name": "Captain", "style": "pirate"})
        assert "Captain" in result[0].text
        assert "Ahoy" in result[0].text

    @pytest.mark.asyncio
    async def test_greet_requires_name(self):
        """Greet should raise error without name."""
        with pytest.raises(ValueError, match="name is required"):
            await call_tool("example_greet", {})

    @pytest.mark.asyncio
    async def test_greet_defaults_to_casual(self):
        """Greet should default to casual style."""
        result = await call_tool("example_greet", {"name": "Test"})
        # Casual style starts with "Hey"
        assert result[0].text.startswith("Hey")


class TestSummarizeTool:
    """Test the example_summarize tool."""

    @pytest.mark.asyncio
    async def test_summarize_returns_markdown(self):
        """Summarize should return markdown with frontmatter."""
        result = await call_tool("example_summarize", {
            "text": "This is a test document with some content.",
            "title": "Test Summary",
        })
        text = result[0].text

        # Should have frontmatter
        assert text.startswith("---")
        assert "date:" in text
        assert "source: lu-example" in text

        # Should have title
        assert "# Test Summary" in text

        # Should have statistics
        assert "Word count:" in text
        assert "Character count:" in text

    @pytest.mark.asyncio
    async def test_summarize_requires_text(self):
        """Summarize should raise error without text."""
        with pytest.raises(ValueError, match="text is required"):
            await call_tool("example_summarize", {})

    @pytest.mark.asyncio
    async def test_summarize_defaults_title(self):
        """Summarize should default to 'Summary' title."""
        result = await call_tool("example_summarize", {
            "text": "Some text here.",
        })
        assert "# Summary" in result[0].text

    @pytest.mark.asyncio
    async def test_summarize_includes_word_count(self):
        """Summarize should count words correctly."""
        result = await call_tool("example_summarize", {
            "text": "one two three four five",
        })
        assert "**Word count:** 5" in result[0].text

    @pytest.mark.asyncio
    async def test_summarize_truncates_long_text(self):
        """Summarize should truncate text over 500 chars."""
        long_text = "word " * 200  # 1000 chars
        result = await call_tool("example_summarize", {
            "text": long_text,
        })
        assert "..." in result[0].text


class TestUnknownTool:
    """Test error handling for unknown tools."""

    @pytest.mark.asyncio
    async def test_unknown_tool_raises(self):
        """Unknown tool should raise ValueError."""
        with pytest.raises(ValueError, match="Unknown tool"):
            await call_tool("nonexistent_tool", {})
