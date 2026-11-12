#!/usr/bin/env python3
"""OpenAI 兼容 LLM 代理：透传请求并把「请求 / 响应 / 耗时 / token 用量 / trace id」落成 JSONL。

T1.4 要求 L3 证据包含「请求/响应/trace/耗时/成本」。直接问模型或看服务端日志都拿不到
完整的 prompt 与 completion，因此在客户端与 provider 之间插一层记录代理：

    sisyphus --config <cfg pointing at this proxy>  →  proxy  →  upstream LLM

用法：
    scripts/l3-llm-proxy.py --port 11500 --upstream http://127.0.0.1:11434/v1/chat/completions \
        --log /tmp/opencode/l3/llm.jsonl

每条记录：
    {"ts","trace_id","model","messages","status","duration_ms","usage","response_content","error"}
"""

import argparse
import itertools
import json
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer


class Config:
    upstream = ""
    log_path = ""
    seq = itertools.count(1)


def _now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%S%z")


def _append(record: dict) -> None:
    with open(Config.log_path, "a", encoding="utf-8") as fh:
        fh.write(json.dumps(record, ensure_ascii=False) + "\n")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(length) if length else b""
        # CLI 目前不发 trace 头；缺失时生成序号 id，保证归档内每次调用可关联。
        seq = next(Config.seq)
        trace_id = (
            self.headers.get("x-trace-id")
            or self.headers.get("x-request-id")
            or f"l3-{seq:03d}"
        )

        try:
            payload = json.loads(raw or b"{}")
        except json.JSONDecodeError:
            payload = {}

        started = time.time()
        record = {
            "ts": _now(),
            "seq": seq,
            "trace_id": trace_id,
            "path": self.path,
            "model": payload.get("model"),
            "messages": payload.get("messages"),
            "json_mode": bool(payload.get("response_format")),
            "max_tokens": payload.get("max_tokens"),
        }

        req = urllib.request.Request(
            Config.upstream,
            data=raw,
            headers={
                "content-type": "application/json",
                "authorization": self.headers.get("authorization", ""),
            },
            method="POST",
        )

        try:
            with urllib.request.urlopen(req, timeout=600) as resp:
                body = resp.read()
                record["status"] = resp.status
        except urllib.error.HTTPError as e:
            body = e.read()
            record["status"] = e.code
        except Exception as e:  # noqa: BLE001 - 记录代理不透传异常类型
            record["status"] = 0
            record["error"] = f"{type(e).__name__}: {e}"
            record["duration_ms"] = int((time.time() - started) * 1000)
            _append(record)
            self.send_response(502)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", "0")
            self.end_headers()
            return

        record["duration_ms"] = int((time.time() - started) * 1000)
        parsed = None
        try:
            parsed = json.loads(body)
        except json.JSONDecodeError:
            record["response_raw"] = body[:2000].decode("utf-8", "replace")

        if isinstance(parsed, dict):
            record["usage"] = parsed.get("usage")
            choices = parsed.get("choices") or []
            if choices:
                msg = choices[0].get("message") or {}
                record["response_content"] = msg.get("content")
            if parsed.get("error"):
                record["error"] = parsed["error"]
        _append(record)

        self.send_response(record["status"])
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=11500)
    ap.add_argument(
        "--upstream",
        default="http://127.0.0.1:11434/v1/chat/completions",
        help="上游 OpenAI 兼容 chat/completions 端点",
    )
    ap.add_argument("--log", required=True, help="JSONL 记录路径")
    args = ap.parse_args()

    Config.upstream = args.upstream
    Config.log_path = args.log
    open(args.log, "a", encoding="utf-8").close()

    HTTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
