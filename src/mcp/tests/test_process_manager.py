"""Tests for MCP process manager."""

import asyncio
import json
import sys
import time
import unittest
from pathlib import Path
from unittest.mock import AsyncMock, MagicMock, patch

# Add parent to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent.parent))

from mcp.process_manager import (
    KEEP_WARM_SECONDS,
    McpProcess,
    ProcessManager,
    get_process_manager,
)


class TestMcpProcess(unittest.TestCase):
    """Tests for McpProcess dataclass."""

    def test_next_request_id_increments(self):
        """Request IDs should increment sequentially."""
        process = MagicMock()
        process.returncode = None
        mcp = McpProcess(name="test", process=process)

        self.assertEqual(mcp.next_request_id(), 1)
        self.assertEqual(mcp.next_request_id(), 2)
        self.assertEqual(mcp.next_request_id(), 3)

    def test_is_running_when_returncode_none(self):
        """Process should be running when returncode is None."""
        process = MagicMock()
        process.returncode = None
        mcp = McpProcess(name="test", process=process)

        self.assertTrue(mcp.is_running)

    def test_is_running_when_exited(self):
        """Process should not be running when returncode is set."""
        process = MagicMock()
        process.returncode = 0
        mcp = McpProcess(name="test", process=process)

        self.assertFalse(mcp.is_running)


class TestProcessManager(unittest.TestCase):
    """Tests for ProcessManager class."""

    def setUp(self):
        """Create a fresh ProcessManager."""
        self.manager = ProcessManager()

    def _create_mock_process(self):
        """Create a mock subprocess."""
        process = AsyncMock()
        process.returncode = None
        # stdin.write() is sync, stdin.drain() is async
        process.stdin = MagicMock()
        process.stdin.write = MagicMock()
        process.stdin.drain = AsyncMock()
        process.stdout = AsyncMock()
        process.stderr = AsyncMock()
        process.terminate = MagicMock()
        process.kill = MagicMock()
        process.wait = AsyncMock()
        return process

    def test_spawn_uses_npx_for_scoped_packages(self):
        """Scoped npm packages should use npx."""

        async def run_test():
            with patch.object(
                asyncio, "create_subprocess_exec", new_callable=AsyncMock
            ) as mock_exec:
                mock_exec.return_value = MagicMock(
                    returncode=None,
                    stdin=AsyncMock(),
                    stdout=AsyncMock(),
                )

                await self.manager._spawn_mcp("@modelcontextprotocol/mcp-server-slack", None)

                # Check npx was called
                call_args = mock_exec.call_args
                self.assertEqual(call_args[0][0], "npx")
                self.assertEqual(call_args[0][1], "-y")
                self.assertIn("@modelcontextprotocol/mcp-server-slack", call_args[0])

        asyncio.run(run_test())

    def test_spawn_uses_uvx_for_python_packages(self):
        """Python packages should use uvx."""

        async def run_test():
            with patch.object(
                asyncio, "create_subprocess_exec", new_callable=AsyncMock
            ) as mock_exec:
                mock_exec.return_value = MagicMock(
                    returncode=None,
                    stdin=AsyncMock(),
                    stdout=AsyncMock(),
                )

                await self.manager._spawn_mcp("mcp-server-memory", None)

                # Check uvx was called
                call_args = mock_exec.call_args
                self.assertEqual(call_args[0][0], "uvx")
                self.assertIn("mcp-server-memory", call_args[0])

        asyncio.run(run_test())

    def test_call_method_sends_jsonrpc(self):
        """call_method should send proper JSON-RPC request."""

        async def run_test():
            mock_process = self._create_mock_process()
            mcp = McpProcess(name="test", process=mock_process)

            # Mock response
            response = {"jsonrpc": "2.0", "id": 1, "result": {"tools": []}}
            mock_process.stdout.readline = AsyncMock(
                return_value=(json.dumps(response) + "\n").encode()
            )

            result = await self.manager.call_method(mcp, "tools/list", {})

            # Verify request was sent
            write_calls = mock_process.stdin.write.call_args_list
            self.assertEqual(len(write_calls), 1)
            sent_data = write_calls[0][0][0].decode()
            sent_request = json.loads(sent_data.strip())

            self.assertEqual(sent_request["jsonrpc"], "2.0")
            self.assertEqual(sent_request["id"], 1)
            self.assertEqual(sent_request["method"], "tools/list")

            # Verify result
            self.assertEqual(result, {"tools": []})

        asyncio.run(run_test())

    def test_call_method_raises_on_error(self):
        """call_method should raise RuntimeError on MCP error."""

        async def run_test():
            mock_process = self._create_mock_process()
            mcp = McpProcess(name="test", process=mock_process)

            # Mock error response
            response = {
                "jsonrpc": "2.0",
                "id": 1,
                "error": {"code": -32600, "message": "Invalid Request"},
            }
            mock_process.stdout.readline = AsyncMock(
                return_value=(json.dumps(response) + "\n").encode()
            )

            with self.assertRaises(RuntimeError) as ctx:
                await self.manager.call_method(mcp, "bad/method", {})

            self.assertIn("Invalid Request", str(ctx.exception))

        asyncio.run(run_test())

    def test_call_method_raises_on_closed_process(self):
        """call_method should raise RuntimeError if MCP closes."""

        async def run_test():
            mock_process = self._create_mock_process()
            mcp = McpProcess(name="test", process=mock_process)

            # Mock empty response (process closed)
            mock_process.stdout.readline = AsyncMock(return_value=b"")

            with self.assertRaises(RuntimeError) as ctx:
                await self.manager.call_method(mcp, "tools/list", {})

            self.assertIn("closed unexpectedly", str(ctx.exception))

        asyncio.run(run_test())

    def test_list_tools(self):
        """list_tools should return tool definitions."""

        async def run_test():
            mock_process = self._create_mock_process()
            mcp = McpProcess(name="test", process=mock_process)

            # Mock response
            tools = [{"name": "read_file", "description": "Read a file"}]
            response = {"jsonrpc": "2.0", "id": 1, "result": {"tools": tools}}
            mock_process.stdout.readline = AsyncMock(
                return_value=(json.dumps(response) + "\n").encode()
            )

            result = await self.manager.list_tools(mcp)

            self.assertEqual(result, tools)

        asyncio.run(run_test())

    def test_call_tool(self):
        """call_tool should invoke the tool and return result."""

        async def run_test():
            mock_process = self._create_mock_process()
            mcp = McpProcess(name="test", process=mock_process)

            # Mock response
            response = {
                "jsonrpc": "2.0",
                "id": 1,
                "result": {"content": [{"type": "text", "text": "file contents"}]},
            }
            mock_process.stdout.readline = AsyncMock(
                return_value=(json.dumps(response) + "\n").encode()
            )

            result = await self.manager.call_tool(mcp, "read_file", {"path": "test.txt"})

            # Verify method params
            write_calls = mock_process.stdin.write.call_args_list
            sent_data = write_calls[0][0][0].decode()
            sent_request = json.loads(sent_data.strip())

            self.assertEqual(sent_request["method"], "tools/call")
            self.assertEqual(sent_request["params"]["name"], "read_file")
            self.assertEqual(sent_request["params"]["arguments"], {"path": "test.txt"})

        asyncio.run(run_test())

    def test_list_running_empty(self):
        """list_running should return empty list initially."""
        self.assertEqual(self.manager.list_running(), [])

    def test_get_process_returns_none_when_not_found(self):
        """get_process should return None for unknown name."""
        self.assertIsNone(self.manager.get_process("unknown"))

    def test_cleanup_idle_stops_old_processes(self):
        """cleanup_idle should stop processes not used recently."""

        async def run_test():
            mock_process = self._create_mock_process()
            mcp = McpProcess(name="test", process=mock_process)
            mcp.last_used = time.time() - KEEP_WARM_SECONDS - 60  # Older than threshold
            self.manager._processes["test"] = mcp

            stopped = await self.manager.cleanup_idle()

            self.assertEqual(stopped, 1)
            self.assertNotIn("test", self.manager._processes)
            mock_process.terminate.assert_called_once()

        asyncio.run(run_test())

    def test_cleanup_idle_keeps_recent_processes(self):
        """cleanup_idle should keep recently used processes."""

        async def run_test():
            mock_process = self._create_mock_process()
            mcp = McpProcess(name="test", process=mock_process)
            mcp.last_used = time.time()  # Just used
            self.manager._processes["test"] = mcp

            stopped = await self.manager.cleanup_idle()

            self.assertEqual(stopped, 0)
            self.assertIn("test", self.manager._processes)

        asyncio.run(run_test())

    def test_stop_terminates_process(self):
        """stop should terminate the specified process."""

        async def run_test():
            mock_process = self._create_mock_process()
            mcp = McpProcess(name="test", process=mock_process)
            self.manager._processes["test"] = mcp

            result = await self.manager.stop("test")

            self.assertTrue(result)
            self.assertNotIn("test", self.manager._processes)
            mock_process.terminate.assert_called_once()

        asyncio.run(run_test())

    def test_stop_returns_false_when_not_found(self):
        """stop should return False for unknown name."""

        async def run_test():
            result = await self.manager.stop("unknown")
            self.assertFalse(result)

        asyncio.run(run_test())

    def test_shutdown_stops_all(self):
        """shutdown should stop all running processes."""

        async def run_test():
            for name in ["mcp1", "mcp2", "mcp3"]:
                mock = self._create_mock_process()
                self.manager._processes[name] = McpProcess(name=name, process=mock)

            await self.manager.shutdown()

            self.assertEqual(len(self.manager._processes), 0)

        asyncio.run(run_test())


class TestGetProcessManager(unittest.TestCase):
    """Tests for global process manager accessor."""

    def test_returns_same_instance(self):
        """get_process_manager should return the same instance."""
        # Reset global
        import mcp.process_manager as pm

        pm._manager = None

        manager1 = get_process_manager()
        manager2 = get_process_manager()

        self.assertIs(manager1, manager2)

        # Clean up
        pm._manager = None


if __name__ == "__main__":
    unittest.main()
