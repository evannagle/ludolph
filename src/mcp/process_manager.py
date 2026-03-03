"""Manages external MCP server subprocesses via stdio JSON-RPC."""

import asyncio
import json
import logging
import os
import time
from dataclasses import dataclass, field
from typing import Any

logger = logging.getLogger(__name__)

KEEP_WARM_SECONDS = 300  # 5 minutes


@dataclass
class McpProcess:
    """A running MCP server subprocess."""

    name: str
    process: asyncio.subprocess.Process
    last_used: float = field(default_factory=time.time)
    _request_id: int = field(default=0, init=False)

    def next_request_id(self) -> int:
        """Generate the next request ID for JSON-RPC."""
        self._request_id += 1
        return self._request_id

    @property
    def is_running(self) -> bool:
        """Check if the process is still running."""
        return self.process.returncode is None


class ProcessManager:
    """Manages MCP server subprocesses."""

    def __init__(self):
        self._processes: dict[str, McpProcess] = {}
        self._lock = asyncio.Lock()

    async def get_or_spawn(
        self,
        name: str,
        package: str,
        env: dict[str, str] | None = None,
    ) -> McpProcess:
        """
        Get running MCP process or spawn new one.

        Args:
            name: Friendly name for the MCP (used as key)
            package: Package name (npm or uvx)
            env: Optional environment variables for the subprocess

        Returns:
            McpProcess instance (running and initialized)
        """
        async with self._lock:
            if name in self._processes:
                proc = self._processes[name]
                proc.last_used = time.time()
                if proc.is_running:
                    return proc
                # Process died, clean up
                del self._processes[name]

            # Spawn new process
            process = await self._spawn_mcp(package, env)
            mcp_proc = McpProcess(name=name, process=process)
            self._processes[name] = mcp_proc

            # Initialize the MCP
            await self._initialize_mcp(mcp_proc)

            return mcp_proc

    async def _spawn_mcp(
        self,
        package: str,
        env: dict[str, str] | None = None,
    ) -> asyncio.subprocess.Process:
        """Spawn an MCP server subprocess."""
        # Determine command based on package format
        if package.startswith("@") or "/" in package:
            # npm package (scoped or namespaced)
            cmd = ["npx", "-y", package]
        else:
            # uvx (Python) package
            cmd = ["uvx", package]

        # Merge environment
        process_env = os.environ.copy()
        if env:
            process_env.update(env)

        logger.info(f"Spawning MCP: {' '.join(cmd)}")

        return await asyncio.create_subprocess_exec(
            *cmd,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=process_env,
        )

    async def _initialize_mcp(self, mcp: McpProcess) -> dict:
        """
        Send initialize request to MCP.

        Args:
            mcp: The MCP process to initialize

        Returns:
            Initialize response from MCP
        """
        return await self.call_method(
            mcp,
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "ludolph", "version": "0.1.0"},
            },
        )

    async def call_method(
        self,
        mcp: McpProcess,
        method: str,
        params: dict[str, Any] | None = None,
    ) -> dict:
        """
        Call a JSON-RPC method on the MCP.

        Args:
            mcp: The MCP process to call
            method: JSON-RPC method name (e.g., "tools/list", "tools/call")
            params: Optional parameters for the method

        Returns:
            Result from the MCP response

        Raises:
            RuntimeError: If MCP closes unexpectedly or returns an error
        """
        mcp.last_used = time.time()
        request_id = mcp.next_request_id()

        request = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
        }
        if params:
            request["params"] = params

        # Send request
        if mcp.process.stdin is None:
            raise RuntimeError(f"MCP {mcp.name} has no stdin")

        request_line = json.dumps(request) + "\n"
        mcp.process.stdin.write(request_line.encode())
        await mcp.process.stdin.drain()

        # Read response
        if mcp.process.stdout is None:
            raise RuntimeError(f"MCP {mcp.name} has no stdout")

        response_line = await mcp.process.stdout.readline()
        if not response_line:
            raise RuntimeError(f"MCP {mcp.name} closed unexpectedly")

        response = json.loads(response_line.decode())

        if "error" in response:
            error = response["error"]
            message = error.get("message", str(error))
            raise RuntimeError(f"MCP error: {message}")

        return response.get("result", {})

    async def list_tools(self, mcp: McpProcess) -> list[dict]:
        """
        Get available tools from an MCP.

        Args:
            mcp: The MCP process to query

        Returns:
            List of tool definitions
        """
        result = await self.call_method(mcp, "tools/list")
        return result.get("tools", [])

    async def call_tool(
        self,
        mcp: McpProcess,
        tool_name: str,
        arguments: dict[str, Any],
    ) -> dict:
        """
        Call a tool on the MCP.

        Args:
            mcp: The MCP process
            tool_name: Name of the tool to call
            arguments: Arguments to pass to the tool

        Returns:
            Result from the tool call
        """
        result = await self.call_method(
            mcp,
            "tools/call",
            {"name": tool_name, "arguments": arguments},
        )
        return result

    def get_process(self, name: str) -> McpProcess | None:
        """
        Get a process by name without spawning.

        Args:
            name: The MCP name

        Returns:
            McpProcess if found and running, None otherwise
        """
        proc = self._processes.get(name)
        if proc and proc.is_running:
            return proc
        return None

    def list_running(self) -> list[str]:
        """
        List names of all running MCP processes.

        Returns:
            List of MCP names that are currently running
        """
        return [name for name, proc in self._processes.items() if proc.is_running]

    async def cleanup_idle(self) -> int:
        """
        Stop MCPs not used recently.

        Returns:
            Count of stopped processes
        """
        async with self._lock:
            now = time.time()
            stopped = 0
            for name in list(self._processes.keys()):
                proc = self._processes[name]
                if now - proc.last_used > KEEP_WARM_SECONDS:
                    logger.info(f"Stopping idle MCP: {name}")
                    await self._terminate_process(proc)
                    del self._processes[name]
                    stopped += 1
            return stopped

    async def stop(self, name: str) -> bool:
        """
        Stop a specific MCP process.

        Args:
            name: The MCP name to stop

        Returns:
            True if stopped, False if not found
        """
        async with self._lock:
            proc = self._processes.get(name)
            if proc is None:
                return False

            logger.info(f"Stopping MCP: {name}")
            await self._terminate_process(proc)
            del self._processes[name]
            return True

    async def _terminate_process(self, proc: McpProcess) -> None:
        """Terminate a process gracefully, then forcefully if needed."""
        proc.process.terminate()
        try:
            await asyncio.wait_for(proc.process.wait(), timeout=5.0)
        except asyncio.TimeoutError:
            proc.process.kill()
            await proc.process.wait()

    async def shutdown(self) -> None:
        """Stop all MCP processes."""
        async with self._lock:
            for name, proc in self._processes.items():
                logger.info(f"Stopping MCP: {name}")
                await self._terminate_process(proc)
            self._processes.clear()


# Global instance
_manager: ProcessManager | None = None


def get_process_manager() -> ProcessManager:
    """Get or create the global process manager."""
    global _manager
    if _manager is None:
        _manager = ProcessManager()
    return _manager
