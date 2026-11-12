#!/usr/bin/env python3
"""桩 LLM（OpenAI 兼容）：每次请求都返回同一份固定响应。

用于**确定性**验收脚本：比如 `sisyphus dev` 的权限闸 —— 需要一个「计划里
带 RunCommand 步骤」的模型输出，用真实模型则不可控（模型可能不生成命令步骤）。

用法：
    scripts/l3-mock-llm.py --port 11510 --content-file plan.json

每次 POST /v1/chat/completions 都返回 `--content-file` 的内容作为 assistant 消息。
"""

import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, HTTPServer


class Config:
    content = "{}"
    model = "mock-llm"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # noqa: N802 - 静默访问日志
        pass

    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(length) if length else b""
        try:
            json.loads(raw or b"{}")
        except json.JSONDecodeError:
            pass

        payload = {
            "id": "mock-1",
            "object": "chat.completion",
            "created": int(time.time()),
            "model": Config.model,
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": Config.content},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10, "total_tokens": 20},
        }
        body = json.dumps(payload).encode("utf-8")
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):  # noqa: N802
        body = b'{"status":"ok"}'
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=11510)
    parser.add_argument("--content-file", required=True)
    parser.add_argument("--model", default="mock-llm")
    args = parser.parse_args()

    with open(args.content_file, "r", encoding="utf-8") as fh:
        Config.content = fh.read()
    Config.model = args.model

    server = HTTPServer(("127.0.0.1", args.port), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
