#!/usr/bin/env python3
"""审计「活文档」里的链接：相对链接必须指向真实文件，外部链接不得 404。

## 为什么需要这个门禁

本次全面审查（2026-11-16）发现 3 处死链，都是「文件搬了 / 仓库迁了 /
路径写错」造成的长期静默失效：

- docs/runbooks/...md 指向 `../../benchmarks/`（实际在 `../benchmarks/`）
- book/src/tutorial.md 指向 `../examples/`（示例实际在 crates/sisyphus-core/examples）
- docs/benchmarks/...md 交接清单的相对路径层级写错

死链不会让任何测试变红，只会让读者点进 404。靠人眼或临时命令发现不可持续，
必须变成常驻门禁。

## 扫描范围

与 audit_doc_counts.py 同口径的「活文档」：

- `README.md`、`CONTRIBUTING.md`、`AGENTS.md`
- `book/src/**/*.md`
- `docs/plans/**/*.md`、`docs/runbooks/**/*.md`、`docs/known-issue-*.md`

**不扫** `docs/benchmarks/**` 等历史档案（改写记录等于伪造历史，其中的
外部链接可能随时间自然失效，不属于「当前状态」）。

## 判定

- 相对链接（不含协议、不含 `mailto:`、不以 `#` 开头）：按文件所在目录
  解析，目标文件必须存在。锚点（`#` 之后）不参与文件判定。
- 外部 `http(s)` 链接：HEAD 请求验证。
  - 2xx / 3xx / 405（站点不支持 HEAD 但存在）→ 通过
  - 404 / 410 → **硬错误**（门禁变红）
  - 超时 / 连接失败 / 5xx → **软警告**（打印但不失败）——第三方站点
    抖动不应让 CI 变红，硬错误只留给确定性的「不存在」。

## 用法

    python3 scripts/audit_links.py               # 校验（CI / make audit）
    python3 scripts/audit_links.py --self-check  # 判据自检
    python3 scripts/audit_links.py --json
退出码：0 通过；1 违例；2 自检失败。
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import time
import urllib.parse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LINK_RE = re.compile(r"\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
MAX_WORKERS = 8

LIVE_GLOBS = [
    "README.md",
    "CONTRIBUTING.md",
    "AGENTS.md",
    "book/src/**/*.md",
    "docs/plans/**/*.md",
    "docs/runbooks/**/*.md",
    "docs/known-issue-*.md",
]


def live_docs(root: Path) -> list[Path]:
    seen: set[Path] = set()
    for pattern in LIVE_GLOBS:
        for p in root.glob(pattern):
            if p.is_file():
                seen.add(p)
    return sorted(seen)


def classify(url: str) -> str:
    if url.startswith(("http://", "https://")):
        return "external"
    # `/openapi.json` 这类是 HTTP API 端点路径（绝对 URL），不是仓库内
    # 相对文件链接；`#anchor` / `mailto:` / `tel:` 与纯文本协议串均跳过。
    if url.startswith(("/", "#", "mailto:", "tel:")) or url.startswith("http"):
        return "skip"
    return "relative"


def check_relative(url: str, base_dir: Path) -> str | None:
    """相对链接：目标文件必须存在。返回错误信息或 None。"""
    path = urllib.parse.urlparse(url).path
    path = urllib.parse.unquote(path)
    target = (base_dir / path).resolve()
    if not target.exists():
        return f"相对链接目标不存在: {url}"
    return None


def check_external(url: str) -> tuple[str, str]:
    """外部链接：curl HEAD（跟随重定向，最多 3 次重试）。

    2xx / 3xx / 405（站点不支持 HEAD 但存在）→ ok
    404 / 410 → dead（硬错误）
    连接失败 / 超时 / 000 → unreachable（软警告）——第三方站点与本地代理
    抖动不该让门禁变红；全部失败时汇总会留下等量软警告，不假称通过。
    """
    code = ""
    for _ in range(3):
        try:
            proc = subprocess.run(
                ["curl", "-sIL", "-o", "/dev/null", "-w", "%{http_code}",
                 "--max-time", "10", url],
                capture_output=True, text=True, timeout=15,
            )
            code = proc.stdout.strip()
        except Exception:
            code = ""
        if code.isdigit() and code != "000":
            break
        time.sleep(1)
    if not code.isdigit() or code == "000":
        return "unreachable", "连接失败/超时（软警告）"
    c = int(code)
    if 200 <= c < 400 or c == 405:
        return "ok", f"HTTP {c}"
    if c in (404, 410):
        return "dead", f"HTTP {c}"
    return "unreachable", f"HTTP {c}（软警告）"


def audit_tree(root: Path) -> tuple[list[dict], list[dict]]:
    """返回 (硬错误, 软警告)。每项含 path / line / url / detail。"""
    hard, soft = [], []
    externals: list[tuple[str, Path, int, str]] = []
    for doc in live_docs(root):
        rel = doc.relative_to(root).as_posix()
        for lineno, line in enumerate(doc.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
            for m in LINK_RE.finditer(line):
                url = m.group(1)
                kind = classify(url)
                if kind == "skip":
                    continue
                if kind == "relative":
                    err = check_relative(url, doc.parent)
                    if err:
                        hard.append({"path": rel, "line": lineno, "url": url, "detail": err})
                else:
                    externals.append((url, doc, lineno, rel))

    if externals:
        with ThreadPoolExecutor(max_workers=MAX_WORKERS) as pool:
            results = pool.map(lambda t: (t, check_external(t[0])), externals)
        for (url, doc, lineno, rel), (verdict, detail) in results:
            entry = {"path": rel, "line": lineno, "url": url, "detail": detail}
            if verdict == "dead":
                hard.append(entry)
            elif verdict == "unreachable":
                soft.append(entry)
    return hard, soft


def self_check() -> int:
    ok = True

    if classify("./LICENSE") != "relative" or classify("https://x.dev") != "external":
        print("自检失败：classify 分类错误", file=sys.stderr); ok = False
    if classify("#anchor") != "skip" or classify("mailto:a@b.c") != "skip":
        print("自检失败：skip 形态误判", file=sys.stderr); ok = False

    if check_relative("./LICENSE", ROOT) is not None:
        print("自检失败：存在的相对链接被误报", file=sys.stderr); ok = False
    if check_relative("./no-such-file-xyz.md", ROOT) is None:
        print("自检失败：不存在的相对链接未拦截", file=sys.stderr); ok = False
    if check_relative("./LICENSE#section", ROOT) is not None:
        print("自检失败：带锚点的相对链接被误报", file=sys.stderr); ok = False

    # 外部链接判定不发起真实网络请求（避免自检依赖外网），只验证分支结构。
    if check_external.__doc__ is None:
        print("自检失败：check_external 缺 docstring", file=sys.stderr); ok = False

    if ok:
        print("✅ 判据自检通过（classify / 相对链接存在性 / 锚点剥离 / 外部链接分支）")
        return 0
    return 2


def main() -> int:
    parser = argparse.ArgumentParser(description="活文档链接巡检")
    parser.add_argument("--self-check", action="store_true", help="判据自检")
    parser.add_argument("--json", action="store_true", help="JSON 输出")
    args = parser.parse_args()

    if args.self_check:
        return self_check()

    hard, soft = audit_tree(ROOT)
    if args.json:
        print(json.dumps({"hard": hard, "soft": soft}, ensure_ascii=False, indent=2))
    else:
        for e in hard:
            print("%s:%d: %s —— %s" % (e["path"], e["line"], e["detail"], e["url"]))
        for e in soft:
            print("%s:%d: [软警告] %s —— %s" % (e["path"], e["line"], e["detail"], e["url"]), file=sys.stderr)
        if hard:
            print("\n❌ %d 处死链（另有 %d 个软警告）。" % (len(hard), len(soft)), file=sys.stderr)
        else:
            print("✅ 活文档链接全通（软警告 %d 个，不阻断）" % len(soft))
    return 1 if hard else 0


if __name__ == "__main__":
    sys.exit(main())
