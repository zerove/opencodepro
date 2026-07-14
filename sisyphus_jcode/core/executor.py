import asyncio
import subprocess
from typing import Any, Dict, Optional, Tuple
from .sop_graph import SOPGraph, NodeKind

class JcodeExecutor:
    """
    Bridges Sisyphus SOP nodes to actual execution in the jcode environment.
    """
    async def execute_node(self, node_name: str, node_kind: NodeKind, config: Dict[str, Any]) -> Tuple[bool, str]:
        print(f"Executing node: {node_name} [{node_kind.value}]")
        
        if node_kind == NodeKind.WASM:
            # In jcode integration, we map WASM tool_id to a bash command or a jcode tool
            tool_id = config.get("tool_id", "unknown")
            return await self._run_tool(tool_id, config)
        
        elif node_kind == NodeKind.TEST:
            command = config.get("command", "true")
            workdir = config.get("workdir", ".")
            return await self._run_bash(command, workdir)
        
        elif node_kind == NodeKind.BRANCH:
            # Branching is handled by the Graph Traversal, not the Node Executor
            return True, "Branch processed"
            
        return False, f"Unsupported node kind: {node_kind}"

    async def _run_tool(self, tool_id: str, config: Dict[str, Any]) -> Tuple[bool, str]:
        # For now, we map tool_id to simple bash commands for prototyping
        # In full integration, this would call jcode's tool registry
        cmd_map = {
            "bash_exec": "echo 'Executing bash...'",
            "file_edit": "echo 'Editing file...'",
            "lsp_search": "echo 'Searching symbols...'",
        }
        cmd = cmd_map.get(tool_id, f"echo 'Tool {tool_id} not mapped'")
        return await self._run_bash(cmd)

    async def _run_bash(self, command: str, workdir: str = ".") -> Tuple[bool, str]:
        try:
            process = await asyncio.create_subprocess_shell(
                command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                cwd=workdir
            )
            stdout, stderr = await process.communicate()
            
            exit_code = process.returncode
            output = stdout.decode() + stderr.decode()
            
            return (exit_code == 0), output
        except Exception as e:
            return False, str(e)
