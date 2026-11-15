#!/usr/bin/env python3
"""TODO 勾选项的**语义退化**检查（ADR-021 决策 1 的可自动化切片）。

## 背景：静态门禁能证明「测试存在」，不能证明「测试在验这件事」

`audit_todo.py` 的判据是静态的：引用的 `path.rs::test_fn` 真实存在、带测试
属性、没被 `#[ignore]`。它能挡住「引用写错」与「勾了却没有任何证据」，但挡不住
ADR-021 列的第 1 类漏网——**证据格式合规而内容空泛**：

- 证据测试的函数体里**一个断言都没有**（或者只剩 `todo!()`）：它跑起来只会
  证明「代码不 panic」，勾选项声称的行为完全没被验证过；
- **跨章节复用同一证据**：不相干的两节勾选项共用同一个测试，至少有一项的
  证据是「借来的」——这正是弱模型批量产出时最常见的凑证据形态。

这两条的共同点是**客观可判**（正则/结构即可判定，无需 LLM 裁决）。ADR-021
明确否决了「全自动 LLM 裁决」（裁决本身不可审计），所以本脚本只做这两条，
其余语义相关性（测试断言内容 ↔ 勾选语义是否真的对得上）仍是人工抽样的事，
脚本不假装覆盖。

## 判定

对每个 `- [x]` 条目引用的测试证据（`path.rs::test_fn`）：

1. **断言信号为空** —— 函数体内既没有任何断言宏（`assert!`/`assert_eq!`/
   `assert_ne!`/`debug_assert*`/`matches!`）、也没有失败期望
   （`#[should_panic]`）、也没有失败即报错的写法（`panic!`/`unreachable!`/
   `.expect(`/`.unwrap*(`/`.is_err()` 等）、签名也不返回 `Result`（`?` 会让
   测试失败）。视为**空壳证据**。
2. **桩实现** —— 函数体里出现 `todo!(` / `unimplemented!(`（还没写）。
3. **跨章节复用** —— 同一个 `path.rs::test_fn` 被**不同章节**里的勾选项引用。
   同一章节下的多个子条目复用同一测试是合法的（描述的是同一组断言），
   因此按 heading 判定，而不是「任意两处引用」。

豁免：条目正文里写 `（语义抽样豁免：理由）` 可跳过该条目的全部检查。
豁免必须带理由，且只能逐条写——在文件头一次性声明会让后面新增的所有条目
自动放行，那正是 ADR-021 说的「静默放行」。

## 范围

只扫 `TODO.md` 的已勾选条目，且只对「静态门禁已判定成立」的测试证据做检查
（引用本身写错的，`audit_todo.py` 会报，不在这里重复）。

用法：
    python3 scripts/audit_todo_semantic.py               # 校验（CI / make audit）
    python3 scripts/audit_todo_semantic.py --self-check  # 判据自检
    python3 scripts/audit_todo_semantic.py --json
退出码：0 通过；1 违例；2 自检失败。
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import audit_todo as at  # noqa: E402  （复用 TODO 解析/证据解析/测试解析）

ROOT = pathlib.Path(__file__).resolve().parent.parent

# 断言信号：命中任一即认为测试有实质校验行为。
ASSERTION_SIGNALS = (
    "assert!",
    "assert_eq!",
    "assert_ne!",
    "assert_matches!",
    "debug_assert",
    "matches!",
    "should_panic",
    "panic!",
    "unreachable!",
    ".expect(",
    ".unwrap(",
    ".unwrap_err(",
    ".unwrap_or_else(",
    ".is_err()",
    ".is_ok()",
    ".is_none()",
    ".is_some()",
)

STUB_SIGNALS = ("todo!(", "unimplemented!(")

WAIVER_RE = re.compile(r"（语义抽样豁免：([^）]*)）")
WAIVER_ALT_RE = re.compile(r"\(语义抽样豁免：([^)]*)\)")


def body_of(path: pathlib.Path, name: str, start_line: int) -> tuple[str, int]:
    """返回 (函数体文本, 起始行号)。用花括号配平截取，缺失时返回空串。"""
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return "", start_line
    i = max(start_line - 1, 0)
    # 跳过属性/签名，找到 fn 行（parse_rust_tests 给的是 fn 行号，通常已是）
    depth = 0
    began = False
    body: list[str] = []
    while i < len(lines):
        line = lines[i]
        body.append(line)
        depth += line.count("{") - line.count("}")
        if "{" in line:
            began = True
        i += 1
        if began and depth <= 0:
            break
    return "\n".join(body), start_line


def has_result_signature(body: str) -> bool:
    """测试签名返回 Result 时，`?` 失败即测试失败——也算实质校验。"""
    head = body.split("{", 1)[0]
    return "-> Result" in head or "-> anyhow::Result" in head or "-> std::io::Result" in head


def classify_test(body: str) -> str | None:
    """返回退化类型；None 表示未发现可自动判定的退化。"""
    if any(sig in body for sig in STUB_SIGNALS):
        return "stub"
    if any(sig in body for sig in ASSERTION_SIGNALS):
        return None
    if has_result_signature(body):
        return None
    return "no_assertion"


def find_owner(
    ref_path: str,
    name: str,
    by_name: dict[str, list[pathlib.Path]],
) -> pathlib.Path | None:
    """定位证据测试所在文件（与 audit_todo 的解析口径一致）。"""
    cite = ref_path.replace("\\", "/")
    base = cite.rsplit("/", 1)[-1]
    candidates = list(by_name.get(base, []))
    exact = [p for p in candidates if p.as_posix().endswith(cite)]
    if exact:
        candidates = exact
    for p in candidates:
        if name in at.parse_rust_tests(p):
            return p
    return None


def audit(root: pathlib.Path = ROOT, todo_name: str = at.TODO_NAME) -> list[dict]:
    """返回问题列表。每项含 kind/reason/detail 便于机器消费。"""
    todo_path = root / todo_name
    if not todo_path.exists():
        return [
            {
                "kind": "missing_todo",
                "reason": "找不到 %s；门禁会静默空转" % todo_path,
                "detail": str(todo_path),
            }
        ]

    items = at.parse_todo(todo_path)
    by_name, fn_index = at.build_index(root)
    harnesses = at.ignored_test_harnesses(root)

    problems: list[dict] = []
    cited: dict[tuple[str, str], dict[str, list[int]]] = {}

    for item in items:
        if not item["checked"]:
            continue
        waiver = WAIVER_RE.search(item["text"]) or WAIVER_ALT_RE.search(item["text"])
        if waiver:
            if not waiver.group(1).strip():
                problems.append(
                    {
                        "kind": "empty_waiver",
                        "reason": "「语义抽样豁免」必须写理由（空理由等于静默放行）",
                        "detail": "TODO.md:%d %s" % (item["line"], item["text"][:80]),
                    }
                )
            continue

        refs, _ = at.extract_evidence(item["text"])
        for ref in refs:
            if ref["kind"] != "test":
                continue
            # 引用本身不成立的由 audit_todo.py 报；这里只查语义。
            if at.resolve_test(ref, root, by_name, fn_index, harnesses, False):
                continue
            owner = find_owner(ref["path"], ref["name"], by_name)
            if owner is None:
                continue
            meta = at.parse_rust_tests(owner)[ref["name"]]
            body, _ = body_of(owner, ref["name"], meta["line"])
            rel = owner.relative_to(root).as_posix()

            kind = classify_test(body)
            if kind is not None:
                reason = (
                    "证据测试是桩（含 todo!/unimplemented!）"
                    if kind == "stub"
                    else "证据测试没有任何断言/失败期望信号（空壳证据）"
                )
                problems.append(
                    {
                        "kind": kind,
                        "reason": reason,
                        "detail": "%s::%s" % (rel, ref["name"]),
                        "todo_line": item["line"],
                    }
                )

            cited.setdefault((rel, ref["name"]), {}).setdefault(item["heading"], []).append(
                item["line"]
            )

    for (rel, name), headings in cited.items():
        if len(headings) < 2:
            continue
        problems.append(
            {
                "kind": "cross_section_reuse",
                "reason": "同一测试被不同章节的勾选项复用（至少一节的证据是借来的）",
                "detail": "%s::%s ← %s"
                % (
                    rel,
                    name,
                    "; ".join("%s (TODO.md:%s)" % (h, ",".join(map(str, ls))) for h, ls in sorted(headings.items())),
                ),
            }
        )

    return problems


def _self_check() -> int:
    """用临时仓库跑正反例：规则空转时自检必须失败。"""
    failures: list[str] = []

    def build(files: dict[str, str]) -> pathlib.Path:
        tmp = pathlib.Path(tempfile.mkdtemp(prefix="todo-semantic-"))
        for rel, content in files.items():
            p = tmp / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding="utf-8")
        return tmp

    def todo(*lines: str) -> str:
        return "# T\n\n## 1 节\n\n" + "\n".join(lines) + "\n"

    src = """pub fn add(a: i32, b: i32) -> i32 { a + b }
#[cfg(test)]
mod tests {
    %s
}
"""

    # 正例 1：证据测试无任何断言 → no_assertion
    empty = src % "".join(
        [
            "#[test]\n    fn test_add() {\n        let _ = super::add(1, 2);\n    }\n",
        ]
    )
    root = build(
        {
            "TODO.md": todo("- [x] 加法可用（无测试：`src/lib.rs::test_add`）"),
            "src/lib.rs": empty,
        }
    )
    got = audit(root)
    if not any(p["kind"] == "no_assertion" for p in got):
        failures.append("空断言测试未被识别：%r" % (got,))

    # 正例 2：桩实现 → stub
    stub = src % "#[test]\n    fn test_add() {\n        todo!(\"later\")\n    }\n"
    root = build(
        {"TODO.md": todo("- [x] 加法可用（`src/lib.rs::test_add`）"), "src/lib.rs": stub}
    )
    got = audit(root)
    if not any(p["kind"] == "stub" for p in got):
        failures.append("桩测试未被识别：%r" % (got,))

    # 正例 3：跨章节复用同一证据 → cross_section_reuse
    ok = src % "#[test]\n    fn test_add() {\n        assert_eq!(super::add(1, 2), 3);\n    }\n"
    root = build(
        {
            "TODO.md": "# T\n\n## 1 节\n\n- [x] 加法可用（`src/lib.rs::test_add`）\n"
            "\n## 2 节\n\n- [x] 减法可用（`src/lib.rs::test_add`）\n",
            "src/lib.rs": ok,
        }
    )
    got = audit(root)
    if not any(p["kind"] == "cross_section_reuse" for p in got):
        failures.append("跨章节复用未被识别：%r" % (got,))

    # 反例 1：有断言的测试不得报
    root = build(
        {"TODO.md": todo("- [x] 加法可用（`src/lib.rs::test_add`）"), "src/lib.rs": ok}
    )
    got = audit(root)
    if got:
        failures.append("有断言的测试被误报：%r" % (got,))

    # 反例 2：同章节下多个子条目复用同一测试是合法的
    same = ok
    root = build(
        {
            "TODO.md": todo(
                "- [x] 加法可用（`src/lib.rs::test_add`）",
                "  - [x] 加法边界（`src/lib.rs::test_add`）",
            ),
            "src/lib.rs": same,
        }
    )
    got = audit(root)
    if got:
        failures.append("同章节复用被误报：%r" % (got,))

    # 反例 3：返回 Result 的测试（`?` 失败即失败）不算空壳
    res = src % "#[test]\n    fn test_add() -> Result<(), String> {\n        let _ = super::add(1, 2);\n        Ok(())\n    }\n"
    root = build(
        {"TODO.md": todo("- [x] 加法可用（`src/lib.rs::test_add`）"), "src/lib.rs": res}
    )
    got = audit(root)
    if got:
        failures.append("Result 签名测试被误报：%r" % (got,))

    # 反例 4：带理由的豁免生效；空理由豁免必须失败
    root = build(
        {
            "TODO.md": todo("- [x] 加法可用（`src/lib.rs::test_add`）（语义抽样豁免：纯性能探针）"),
            "src/lib.rs": empty,
        }
    )
    if audit(root):
        failures.append("带理由的豁免未生效：%r" % (audit(root),))
    root = build(
        {
            "TODO.md": todo("- [x] 加法可用（`src/lib.rs::test_add`）（语义抽样豁免：）"),
            "src/lib.rs": empty,
        }
    )
    if not any(p["kind"] == "empty_waiver" for p in audit(root)):
        failures.append("空理由豁免未被拦下")

    # 反例 5：引用写错的条目不在这里重复报（交给 audit_todo.py）
    root = build(
        {"TODO.md": todo("- [x] 加法可用（`src/lib.rs::test_missing`）"), "src/lib.rs": ok}
    )
    if audit(root):
        failures.append("不存在的引用于本脚本报错（应交给 audit_todo.py）：%r" % (audit(root),))

    # 规则表非空 + 真实仓库能扫到条目（否则门禁静默空转）
    if not ASSERTION_SIGNALS:
        failures.append("ASSERTION_SIGNALS 为空：门禁会静默通过")
    real = ROOT / at.TODO_NAME
    if real.exists() and not any(i["checked"] for i in at.parse_todo(real)):
        failures.append("真实 TODO.md 里没有已勾选条目：扫描可能失效")

    if failures:
        for f in failures:
            print("❌ " + f, file=sys.stderr)
        return 2
    print("✅ 判据自检通过（3 组正例 / 5 组反例 + 规则表与真实扫描）")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="TODO 勾选项语义退化检查（ADR-021）")
    parser.add_argument("--self-check", action="store_true", help="只跑判据自检")
    parser.add_argument("--json", action="store_true", help="输出 JSON")
    args = parser.parse_args()

    if args.self_check:
        return _self_check()

    problems = audit(ROOT)

    if args.json:
        print(json.dumps({"problems": problems}, ensure_ascii=False, indent=2))
    else:
        for p in problems:
            print("%s: %s —— %s" % (p["kind"], p["reason"], p.get("detail", "")))
        if problems:
            print(
                "\n❌ %d 处语义退化（ADR-021：绝不静默放行）。补实断言、拆开借用证据，"
                "或逐条写「（语义抽样豁免：理由）」。" % len(problems),
                file=sys.stderr,
            )
        else:
            print("✅ 已勾选条目的证据测试无空壳/桩/跨章节借用（范围见模块 docstring）")

    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
