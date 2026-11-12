#!/usr/bin/env python3
"""最小 MCP 服务器（stdio 传输，newline-delimited JSON）。

这是 L3「MCP 场景」的**真实服务端**，不是 mock：它按 MCP 协议实现
`initialize` / `notifications/initialized` / `tools/list` / `tools/call`，
用来验证 sisyphus 的 MCP 客户端能连上一个**独立实现**的服务器并正确调用工具。

用法：
    python3 scripts/l3-mcp-server.py            # 从 stdin 读 JSON-RPC，写 stdout

工具：
    sum(a: number, b: number)        → 两数之和
    word_count(text: string)         → 词数
    echo(text: string)               → 原样回显

协议细节（与 MCP stdio 传输一致）：
- 每个请求/响应是一行 JSON（以 `\\n` 分隔）；
- 通知（没有 `id`）不回复；
- `initialize` 返回 protocolVersion / capabilities / serverInfo。
"""

import json
import sys

PROTOCOL_VERSION = "2025-03-26"
SERVER_NAME = "l3-mcp-fixture"
SERVER_VERSION = "0.1.0"

TOOLS = [
    {
        "name": "sum",
        "description": "两数求和",
        "inputSchema": {
            "type": "object",
            "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
            "required": ["a", "b"],
        },
    },
    {
        "name": "word_count",
        "description": "统计文本词数",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        },
    },
    {
        "name": "echo",
        "description": "原样回显文本",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        },
    },
]

RESOURCES = [
    {
        "uri": "memo://welcome",
        "name": "welcome",
        "description": "欢迎文本资源",
        "mimeType": "text/plain",
    },
    {
        "uri": "memo://status",
        "name": "status",
        "description": "状态资源",
        "mimeType": "text/plain",
    },
]

PROMPTS = [
    {
        "name": "greet",
        "description": "生成问候语",
        "arguments": [
            {"name": "name", "description": "名字", "required": True},
        ],
    },
    {
        "name": "summary",
        "description": "生成摘要提示",
        "arguments": [],
    },
]


def call_tool(name, args):
    """执行工具；未知工具/参数缺失抛 ValueError（客户端会收到 isError=true）。"""
    if name == "sum":
        a, b = args.get("a"), args.get("b")
        if not isinstance(a, (int, float)) or not isinstance(b, (int, float)):
            raise ValueError("sum 需要数字参数 a / b")
        return str(a + b)
    if name == "word_count":
        text = args.get("text")
        if not isinstance(text, str):
            raise ValueError("word_count 需要字符串参数 text")
        return str(len(text.split()))
    if name == "echo":
        text = args.get("text")
        if not isinstance(text, str):
            raise ValueError("echo 需要字符串参数 text")
        return text
    raise ValueError(f"unknown tool: {name}")


def handle(msg):
    """返回响应 dict；通知（无 id）返回 None。"""
    method = msg.get("method")
    mid = msg.get("id")
    params = msg.get("params") or {}

    if method == "initialize":
        result = {
            "protocolVersion": params.get("protocolVersion", PROTOCOL_VERSION),
            "capabilities": {
                "tools": {"listChanged": True},
                "resources": {"subscribe": False, "listChanged": False},
                "prompts": {"listChanged": False},
            },
            "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
        }
    elif method == "tools/list":
        result = {"tools": TOOLS}
    elif method == "tools/call":
        try:
            text = call_tool(params.get("name"), params.get("arguments") or {})
            result = {"content": [{"type": "text", "text": text}], "isError": False}
        except ValueError as exc:
            result = {"content": [{"type": "text", "text": str(exc)}], "isError": True}
    elif method == "resources/list":
        result = {"resources": RESOURCES}
    elif method == "resources/read":
        uri = params.get("uri")
        body = {
            "memo://welcome": "hello from mcp resource",
            "memo://status": "status: ok",
        }.get(uri)
        if body is None:
            return {
                "jsonrpc": "2.0",
                "id": mid,
                "error": {"code": -32002, "message": f"resource not found: {uri}"},
            }
        result = {
            "contents": [
                {"uri": uri, "mimeType": "text/plain", "text": body},
            ]
        }
    elif method == "prompts/list":
        result = {"prompts": PROMPTS}
    elif method == "prompts/get":
        name = params.get("name")
        args = params.get("arguments") or {}
        if name == "greet":
            who = args.get("name", "world")
            result = {
                "description": "greeting prompt",
                "messages": [
                    {
                        "role": "user",
                        "content": {"type": "text", "text": f"Say hello to {who}"},
                    }
                ],
            }
        elif name == "summary":
            result = {
                "description": "summary prompt",
                "messages": [
                    {
                        "role": "user",
                        "content": {"type": "text", "text": "Summarize the context."},
                    }
                ],
            }
        else:
            return {
                "jsonrpc": "2.0",
                "id": mid,
                "error": {"code": -32602, "message": f"prompt not found: {name}"},
            }
    else:
        return {
            "jsonrpc": "2.0",
            "id": mid,
            "error": {"code": -32601, "message": f"method not found: {method}"},
        }

    return {"jsonrpc": "2.0", "id": mid, "result": result}


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            sys.stdout.write(
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": None,
                        "error": {"code": -32700, "message": "parse error"},
                    }
                )
                + "\n"
            )
            sys.stdout.flush()
            continue

        response = handle(msg)
        if response is None or msg.get("id") is None:
            # 通知无需回复
            continue
        sys.stdout.write(json.dumps(response, ensure_ascii=False) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
