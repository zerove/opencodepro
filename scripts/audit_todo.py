#!/usr/bin/env python3
"""审计 TODO.md 里「已勾选」的条目是否附带了可验证证据。

背景：本项目的失败模式不是「写错了」，而是**「看起来完成了」**——TODO 被勾上、
测试名被引用、CI 全绿，但生产路径上从未生效；或者引用的测试根本不存在、早已
改名，或者带 `#[ignore]` 从来没跑过。阶段 10 的结论是：

    勾选状态不能靠人记，必须变成可验证的门禁。

所以本审计对每个 `- [x]` 条目做两件事：

1. **证据必须能解析**。条目里出现的 `path/to/file.rs::test_name` 必须真的能在
   仓库里找到该文件与函数，且函数带测试属性、未被 `#[ignore]`；`make <target>`
   必须真的在 Makefile 里定义；`scripts/x.py` 必须存在。引用写错、文件改名、
   测试被删，都会在这里暴露。
2. **新增勾选项必须附证据**。没有证据、也没写显式豁免的勾选项会被拦下。

`#[ignore]` 的例外：若该测试文件在某个 workflow 里以 `--ignored` 显式执行
（例如 `toolchain-e2e` job 跑 `language_backend_guards`），它确实在 CI 里跑，
允许作为证据。判据是 workflow 文本里同时出现 `--ignored` 与 `--test <名>`，
或条目正文自己写明 `--ignored`。

**历史基线**：落地本门禁时已有百余条勾选项没有任何证据——它们不可能凭空补出
证据，硬补只会制造新的「看起来完成」。这些条目登记在
`scripts/todo_baseline.txt` 里豁免（ratchet，只减不增）。新增条目一律按上面的
规则校验；改动历史条目的文字会让它「变成新条目」，从而要求补证据——这是刻意
的：动了一条旧结论，就该重新确认它。

豁免写法（确无自动化测试时，例如纯文档 / CI 配置改动）：

    （无测试：reason）

用法：
    python3 scripts/audit_todo.py                    # 校验（CI）
    python3 scripts/audit_todo.py --self-check       # 判据自检
    python3 scripts/audit_todo.py --update-baseline  # 重新登记历史豁免
    python3 scripts/audit_todo.py --json
退出码：0 通过；1 违例；2 自检失败。
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
TODO_NAME = "TODO.md"
BASELINE_NAME = "scripts/todo_baseline.txt"

ITEM_RE = re.compile(r"^(?P<indent>\s*)- \[(?P<mark>[ xX])\]\s+(?P<text>.*)$")
HEADING_RE = re.compile(r"^#{2,4}\s+(?P<title>.*)$")

# `path/to/file.rs::test_name`（可省略目录，按 basename 解析）
TEST_REF_RE = re.compile(r"([A-Za-z0-9_][A-Za-z0-9_./-]*\.rs)::([A-Za-z_][A-Za-z0-9_]*)")
# 反引号里的 `make <target>`；不用裸文本匹配，避免英文 "make sure" 误判
MAKE_REF_RE = re.compile(r"`\s*make\s+([A-Za-z0-9][A-Za-z0-9_.-]*)")
# 反引号里的 `scripts/x.py`（含 `python3 scripts/x.py --self-check`）
SCRIPT_REF_RE = re.compile(r"`[^`]*?\b(scripts/[A-Za-z0-9_]+\.py)\b")
# 显式豁免：（无测试：理由） / (no-test: reason)
EXEMPT_RE = re.compile(r"[（(]\s*(?:无测试|no-test)\s*[:：]\s*([^）)]*?)\s*[）)]")

FN_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?"
    r'(?:(?:async|const|unsafe|extern(?:\s+"[^"]*")?)\s+)*'
    r"fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[<(]"
)
MAKE_TARGET_RE = re.compile(r"^([A-Za-z0-9][A-Za-z0-9_.-]*)\s*:(?!=)")
PHONY_RE = re.compile(r"^\.PHONY\s*:\s*(.*)$")

SKIP_DIRS = {".git", "target", "node_modules", ".venv", "__pycache__", "data"}


def normalize(text: str) -> str:
    """基线与条目比对用的规范化形式：折叠空白。"""
    return " ".join(text.split())


def parse_todo(path: pathlib.Path) -> list[dict]:
    """抽出所有 checkbox 条目（含缩进的子条目）。"""
    items: list[dict] = []
    heading = ""
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        h = HEADING_RE.match(line)
        if h:
            heading = h.group("title").strip()
            continue
        m = ITEM_RE.match(line)
        if not m:
            continue
        items.append(
            {
                "line": lineno,
                "checked": m.group("mark").lower() == "x",
                "indent": len(m.group("indent").expandtabs(4)),
                "text": m.group("text").strip(),
                "heading": heading,
            }
        )
    return items


def rust_sources(root: pathlib.Path) -> list[pathlib.Path]:
    out = []
    for p in root.rglob("*.rs"):
        if any(part in SKIP_DIRS for part in p.relative_to(root).parts):
            continue
        out.append(p)
    return out


def parse_rust_tests(path: pathlib.Path) -> dict[str, dict]:
    """返回 {fn 名: {line, is_test, ignored}}。

    属性块按「向前累积」解析：`#[tokio::test(...)]` 可能跨多行，合并到同一块，
    避免把 `#[test(\\n    ...\\n)]` 误判成非测试函数。
    """
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return {}

    found: dict[str, dict] = {}
    pending: list[str] = []
    depth = 0  # 未闭合的属性方括号深度

    for idx, raw in enumerate(lines, 1):
        stripped = raw.strip()
        if not stripped:
            continue
        # 注释不打断属性与函数之间的绑定
        if (
            stripped.startswith("//")
            or stripped.startswith("/*")
            or stripped.startswith("*")
        ):
            continue
        if stripped.startswith("#[") or stripped.startswith("#!["):
            pending.append(stripped)
            depth += stripped.count("[") - stripped.count("]")
            continue
        if depth > 0:
            pending[-1] += " " + stripped
            depth += stripped.count("[") - stripped.count("]")
            continue

        m = FN_RE.match(raw)
        if m:
            attrs = " ".join(pending).lower()
            found.setdefault(
                m.group(1),
                {
                    "line": idx,
                    "is_test": "test" in attrs,
                    "ignored": "ignore" in attrs,
                },
            )
        pending.clear()
        depth = 0

    return found


def makefile_targets(root: pathlib.Path) -> set[str]:
    targets: set[str] = set()
    path = root / "Makefile"
    if not path.exists():
        return targets
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        m = MAKE_TARGET_RE.match(line)
        if m:
            targets.add(m.group(1))
        p = PHONY_RE.match(line)
        if p:
            targets.update(p.group(1).split())
    return targets


def ignored_test_harnesses(root: pathlib.Path) -> set[str]:
    """在 workflow 里被 `--ignored` 显式执行的 test target 名（文件级）。"""
    allowed: set[str] = set()
    wf_dir = root / ".github" / "workflows"
    if not wf_dir.is_dir():
        return allowed
    for wf in wf_dir.glob("*.yml"):
        text = wf.read_text(encoding="utf-8", errors="replace")
        if "--ignored" not in text:
            continue
        for m in re.finditer(r"--test\s+([A-Za-z0-9_]+)", text):
            allowed.add(m.group(1))
    return allowed


def extract_evidence(text: str) -> tuple[list[dict], str | None]:
    evidence: list[dict] = []
    for m in TEST_REF_RE.finditer(text):
        evidence.append({"kind": "test", "path": m.group(1), "name": m.group(2)})
    for m in MAKE_REF_RE.finditer(text):
        evidence.append({"kind": "make", "target": m.group(1)})
    for m in SCRIPT_REF_RE.finditer(text):
        evidence.append({"kind": "script", "path": m.group(1)})
    ex = EXEMPT_RE.search(text)
    return evidence, (ex.group(1).strip() if ex else None)


def load_baseline(path: pathlib.Path) -> list[str]:
    if not path.exists():
        return []
    entries = []
    for line in path.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        entries.append(s)
    return entries


def build_index(
    root: pathlib.Path,
) -> tuple[dict[str, list[pathlib.Path]], dict[str, list[dict]]]:
    by_name: dict[str, list[pathlib.Path]] = {}
    fn_index: dict[str, list[dict]] = {}
    for p in rust_sources(root):
        rel = p.relative_to(root)
        by_name.setdefault(p.name, []).append(p)
        for fn, meta in parse_rust_tests(p).items():
            fn_index.setdefault(fn, []).append({"path": p, "rel": str(rel), **meta})
    return by_name, fn_index


def resolve_test(
    ref: dict,
    root: pathlib.Path,
    by_name: dict[str, list[pathlib.Path]],
    fn_index: dict[str, list[dict]],
    ignored_harnesses: set[str],
    allow_ignored_inline: bool,
) -> str | None:
    """返回错误描述；None 表示证据成立。"""
    cite = ref["path"].replace("\\", "/")
    name = ref["name"]
    base = cite.rsplit("/", 1)[-1]

    candidates = list(by_name.get(base, []))
    exact = [p for p in candidates if p.relative_to(root).as_posix().endswith(cite)]
    if exact:
        candidates = exact

    if not candidates:
        hint = ""
        if name in fn_index:
            hint = "；同名测试在 " + ", ".join(
                f"{i['rel']}:{i['line']}" for i in fn_index[name]
            )
        return f"引用的文件不存在：{cite}{hint}"

    tests = {p: parse_rust_tests(p) for p in candidates}
    owning = [p for p, t in tests.items() if name in t]
    if not owning:
        hint = ""
        if name in fn_index:
            hint = "；该测试实际位于 " + ", ".join(
                f"{i['rel']}:{i['line']}" for i in fn_index[name]
            )
        files = ", ".join(str(p.relative_to(root)) for p in candidates)
        return f"文件 {files} 里没有 fn {name}(){hint}"

    if len(owning) > 1:
        files = ", ".join(str(p.relative_to(root)) for p in owning)
        return f"引用有歧义：{name} 同时存在于 {files}，请写全路径"

    owner = owning[0]
    meta = tests[owner][name]
    if not meta["is_test"]:
        return (
            f"{owner.relative_to(root)}:{meta['line']} 的 {name} 不是测试函数"
            "（缺 #[test] 属性）"
        )
    if meta["ignored"]:
        if allow_ignored_inline or owner.stem in ignored_harnesses:
            return None
        return (
            f"{owner.relative_to(root)}:{meta['line']} 的 {name} 带 #[ignore]，"
            "CI 默认不执行；若已接入 --ignored job 请在条目中写明 --ignored"
        )
    return None


def audit(
    root: pathlib.Path,
    todo_name: str = TODO_NAME,
    baseline_name: str = BASELINE_NAME,
) -> dict:
    items = parse_todo(root / todo_name)
    targets = makefile_targets(root)
    by_name, fn_index = build_index(root)
    ignored_harnesses = ignored_test_harnesses(root)
    baseline = set(load_baseline(root / baseline_name))

    problems: list[dict] = []
    invalid_evidence: list[dict] = []
    stats = {"checked": 0, "open": 0, "verified": 0, "exempt": 0, "grandfathered": 0}
    checked_texts: set[str] = set()

    for item in items:
        if not item["checked"]:
            stats["open"] += 1
            continue
        stats["checked"] += 1
        normalized = normalize(item["text"])
        checked_texts.add(normalized)
        evidence, exemption = extract_evidence(item["text"])
        allow_ignored_inline = "--ignored" in item["text"]

        errors: list[str] = []
        for ref in evidence:
            if ref["kind"] == "test":
                err = resolve_test(
                    ref,
                    root,
                    by_name,
                    fn_index,
                    ignored_harnesses,
                    allow_ignored_inline,
                )
            elif ref["kind"] == "make":
                err = None if ref["target"] in targets else f"Makefile 里没有目标 {ref['target']}"
            else:
                err = None if (root / ref["path"]).exists() else f"脚本不存在：{ref['path']}"
            if err:
                errors.append(err)

        if errors:
            problem = {"item": item, "errors": errors}
            problems.append(problem)
            invalid_evidence.append(problem)
            continue

        if evidence:
            stats["verified"] += 1
            continue

        if exemption is not None:
            if not exemption:
                problem = {"item": item, "errors": ["豁免标记（无测试：…）的理由为空"]}
                problems.append(problem)
                invalid_evidence.append(problem)
            else:
                stats["exempt"] += 1
            continue

        if normalized in baseline:
            stats["grandfathered"] += 1
            continue

        problems.append(
            {
                "item": item,
                "errors": [
                    "已勾选但没有任何可验证证据：请附 `tests/foo.rs::test_name`"
                    "（须真实存在、带测试属性、未被 #[ignore]）、`make <target>`、"
                    "`scripts/x.py`；确无自动化测试时写（无测试：理由）"
                ],
            }
        )

    stale = sorted(baseline - checked_texts)
    return {
        "stats": stats,
        "problems": problems,
        "invalid_evidence": invalid_evidence,
        "stale": stale,
        "baseline": baseline,
    }


# ── 自检 ────────────────────────────────────────────────────────────────


def _self_check() -> list[str]:
    """用合成仓库验证判别力：直接调用生产函数，不复制判定逻辑。"""
    failures: list[str] = []

    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / "scripts").mkdir()
        (root / ".github" / "workflows").mkdir(parents=True)
        (root / "crates" / "a" / "tests").mkdir(parents=True)

        (root / "Makefile").write_text(
            "good-target:\n\t@true\n.PHONY: good-target\n", encoding="utf-8"
        )
        (root / "scripts" / "ok.py").write_text("# probe\n", encoding="utf-8")
        # foo.rs 由带 --ignored 的 workflow 显式执行 → 其中的 ignored 测试算证据
        (root / "crates" / "a" / "tests" / "foo.rs").write_text(
            "\n".join(
                [
                    "#[test]",
                    "fn real_test() {}",
                    "",
                    '#[tokio::test(flavor = "multi_thread")]',
                    "async fn multiline_attr_test() {}",
                    "",
                    '#[ignore = "requires real mvn + jdk"]',
                    "#[test]",
                    "fn ignored_by_harness() {}",
                    "",
                    "fn helper() {}",
                ]
            ),
            encoding="utf-8",
        )
        # bar.rs 不在任何 --ignored job 里 → 其中的 ignored 测试不算证据
        (root / "crates" / "a" / "tests" / "bar.rs").write_text(
            "\n".join(
                [
                    '#[ignore = "requires network"]',
                    "#[test]",
                    "fn ignored_test() {}",
                    "",
                    "fn unused() {}",
                ]
            ),
            encoding="utf-8",
        )
        (root / ".github" / "workflows" / "ci.yml").write_text(
            "run: cargo test --test foo -- --ignored --test-threads=1\n",
            encoding="utf-8",
        )

        def run(todo: str, baseline: str = "") -> dict:
            (root / "TODO.md").write_text(todo, encoding="utf-8")
            (root / "scripts" / "todo_baseline.txt").write_text(
                baseline, encoding="utf-8"
            )
            return audit(root)

        def expect_clean(label: str, todo: str, baseline: str = "") -> None:
            r = run(todo, baseline)
            if r["problems"]:
                failures.append(f"{label}: 被误判 {r['problems']}")

        def expect_blocked(label: str, todo: str) -> None:
            if not run(todo)["problems"]:
                failures.append(f"{label}: 未拦下")

        expect_clean("有效测试引用", "- [x] 有测试 `crates/a/tests/foo.rs::real_test`\n")
        expect_clean("跨行属性", "- [x] 多行属性 `foo.rs::multiline_attr_test`\n")
        expect_blocked("不存在的测试函数", "- [x] 假测试 `foo.rs::no_such_test`\n")
        expect_blocked("不存在的文件", "- [x] 假文件 `nope.rs::real_test`\n")
        expect_blocked("非测试函数", "- [x] 辅助函数 `foo.rs::helper`\n")
        expect_blocked("未接入 job 的 #[ignore] 测试", "- [x] 忽略 `bar.rs::ignored_test`\n")
        expect_clean(
            "接入 --ignored job 的测试", "- [x] harness `foo.rs::ignored_by_harness`\n"
        )
        expect_clean(
            "make 与 script 证据",
            "- [x] 门禁 `make good-target` 与 `scripts/ok.py --self-check`\n",
        )
        expect_blocked("不存在的 make 目标", "- [x] 假目标 `make no-such-target`\n")
        expect_blocked("无证据的新条目", "- [x] 光秃秃的勾选\n")
        expect_clean("基线豁免", "- [x] 光秃秃的勾选\n", baseline="光秃秃的勾选\n")
        expect_clean("显式豁免", "- [x] 纯文档（无测试：只改 markdown）\n")
        expect_blocked("空豁免理由", "- [x] 纯文档（无测试：）\n")
        expect_clean("未勾选条目不校验", "- [ ] 未来测试 `foo.rs::future_test`\n")

        r = run("- [x] 有测试 `foo.rs::real_test`\n", baseline="已经删掉的条目\n")
        if not r["stale"]:
            failures.append("失效基线条目未被报告")

    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--self-check", action="store_true")
    parser.add_argument("--update-baseline", action="store_true")
    args = parser.parse_args()

    self_failures = _self_check()
    if self_failures:
        print("❌ 判据自检失败：", file=sys.stderr)
        for f in self_failures:
            print(f"  - {f}", file=sys.stderr)
        return 2
    if args.self_check:
        print("✅ 自检通过（14 个判别用例）")
        return 0

    result = audit(ROOT)
    stats = result["stats"]
    problems = result["problems"]

    if args.update_baseline:
        # 只拒绝「证据写错」；纯粹的「没证据」允许重新登记（ratchet 的维护入口）。
        if result["invalid_evidence"]:
            print("❌ 存在无法解析的证据，先修掉再更新基线：", file=sys.stderr)
            for p in result["invalid_evidence"]:
                item = p["item"]
                print(f"  TODO.md:{item['line']}  {item['text'][:80]}", file=sys.stderr)
                for e in p["errors"]:
                    print(f"      {e}", file=sys.stderr)
            return 1
        entries = sorted(
            {
                normalize(i["text"])
                for i in parse_todo(ROOT / TODO_NAME)
                if i["checked"]
                and not extract_evidence(i["text"])[0]
                and extract_evidence(i["text"])[1] is None
            }
        )
        header = (
            "# TODO 勾选门禁的历史豁免（scripts/audit_todo.py）\n"
            "#\n"
            "# 落地门禁之前就已勾选、且没有可验证证据的条目。它们不证明任何事，\n"
            "# 只是「不追溯」，避免为了过门禁而编造证据。只减不增：新增条目必须\n"
            "# 附真实测试名 / make 目标 / 脚本，或写明（无测试：理由）。\n"
            "# 重新生成：python3 scripts/audit_todo.py --update-baseline\n"
        )
        (ROOT / BASELINE_NAME).write_text(
            header + "\n".join(entries) + "\n", encoding="utf-8"
        )
        print(f"✅ 已登记 {len(entries)} 条历史豁免 → {BASELINE_NAME}")
        return 0

    if args.json:
        print(
            json.dumps(
                {
                    "stats": stats,
                    "problems": [
                        {
                            "line": p["item"]["line"],
                            "text": p["item"]["text"],
                            "heading": p["item"]["heading"],
                            "errors": p["errors"],
                        }
                        for p in problems
                    ],
                    "stale_baseline": result["stale"],
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return 1 if problems else 0

    print(
        f"TODO 勾选门禁：{stats['checked']} 条勾选 / {stats['open']} 条未勾选"
        f"（可验证 {stats['verified']}、显式豁免 {stats['exempt']}、"
        f"历史基线 {stats['grandfathered']}）"
    )
    if result["stale"]:
        print(
            f"ℹ️  基线中 {len(result['stale'])} 条已无对应勾选项"
            "（条目被删除或改写过），可运行 --update-baseline 重新登记"
        )

    if not problems:
        print("✅ 所有勾选项的证据均成立")
        return 0

    print(f"\n❌ {len(problems)} 条勾选项的证据不成立：\n")
    for p in problems:
        item = p["item"]
        print(f"  TODO.md:{item['line']}  {item['text'][:100]}")
        if item["heading"]:
            print(f"      所属小节：{item['heading']}")
        for e in p["errors"]:
            print(f"      - {e}")
    print(
        "\n勾选 = 声称「已验证」。证据必须是能跑的东西：真实存在且未被 #[ignore]"
        " 的测试、真实存在的 make 目标或脚本；\n确无自动化测试时写（无测试：理由），"
        "而不是让勾选本身充当证据。\n"
        "注意：条目里的示例写法也会被当成引用，写示例时请避开 `*.rs::fn` 形式。"
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
