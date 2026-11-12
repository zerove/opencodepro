#!/usr/bin/env python3
"""T3.2 失败路径验证用的最小 OpenAI 兼容 mock 服务。

只覆盖 `provider test` 需要区分的失败分支：
  POST /401/v1/chat/completions   -> 401 + OpenAI 风格错误体
  POST /403/v1/chat/completions   -> 403
  POST /404/v1/chat/completions   -> 404
  POST /html/v1/chat/completions  -> 200 但返回 HTML（协议选错）
  POST /ok/v1/chat/completions    -> 200 正常 completion
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

ERRORS = {
    "/401/v1/chat/completions": 401,
    "/403/v1/chat/completions": 403,
    "/404/v1/chat/completions": 404,
}


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("content-length") or 0)
        if length:
            self.rfile.read(length)

        if self.path in ERRORS:
            status = ERRORS[self.path]
            body = json.dumps(
                {"error": {"message": f"mock error {status}", "type": "invalid_request_error"}}
            ).encode()
            self.send_response(status)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        if self.path.startswith("/html/"):
            body = b"<html><body>not an api</body></html>"
            self.send_response(200)
            self.send_header("content-type", "text/html")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        body = json.dumps(
            {
                "id": "mock",
                "object": "chat.completion",
                "model": "mock-model",
                "choices": [
                    {"index": 0, "message": {"role": "assistant", "content": "MOCK-OK"}}
                ],
            }
        ).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 15999
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()
