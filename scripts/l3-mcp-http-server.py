#!/usr/bin/env python3
"""最小 MCP 服务器（Streamable HTTP 传输）—— L3 HTTP 传输场景的服务端。

stdio 版本见 `l3-mcp-server.py`；这里复用它的工具实现（真实 MCP 协议，
不复用本仓 Rust 代码），只把传输换成 HTTP：

- `POST /mcp`：请求体是 JSON-RPC 消息。
  - 带 `id` 的请求 → `200 application/json`，响应体是 JSON-RPC 响应；
  - 没有 `id` 的**通知** → `202 Accepted`，**空响应体**（协议规定通知无响应；
    若回一个响应体，客户端的下一个请求就会读到错位的响应）。
- `GET /mcp` → `405`（本 fixture 不提供 SSE 流）。

用法：
    python3 scripts/l3-mcp-http-server.py --port 11520
"""

import argparse
import importlib.util
import json
import pathlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# 复用 stdio fixture 的协议实现（工具定义 + 方法分派）
_spec = importlib.util.spec_from_file_location(
    "l3_mcp_server", pathlib.Path(__file__).with_name("l3-mcp-server.py")
)
_stdio = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_stdio)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # noqa: N802 - 静默访问日志
        pass

    def _send(self, code, body=b"", content_type="application/json"):
        self.send_response(code)
        if body:
            self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(length) if length else b""

        try:
            msg = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            self._send(
                200,
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": None,
                        "error": {"code": -32700, "message": "parse error"},
                    }
                ).encode("utf-8"),
            )
            return

        response = _stdio.handle(msg)
        if response is None or msg.get("id") is None:
            # 通知：202 + 空体（不返回任何 JSON-RPC 响应）
            self._send(202)
            return

        self._send(200, json.dumps(response, ensure_ascii=False).encode("utf-8"))

    def do_GET(self):  # noqa: N802
        # 本 fixture 不提供 SSE 流
        self._send(405)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=11520)
    parser.add_argument("--host", default="127.0.0.1")
    args = parser.parse_args()

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
