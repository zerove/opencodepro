#!/usr/bin/env python3
"""审计 `#[ignore]` 的理由是否诚实。

背景：本项目有 24 处 `#[ignore]`（实测，全在 crates/ 下），而 CI 从不使用
`--include-ignored`——被忽略的测试等于没有覆盖。其中两处写着
`#[ignore = "requires dedicated runner"]`，但它们失败的原因不是资源受限，
而是内核未注入、失败被吞成成功。把「测试红了」写成「环境受限」，会让真正
的回归保护凭空消失且无人察觉。

**2026-10-08 更正**：原文写「15 处」，实测为 24 处。这个数字从写下后就没
再核对过，测试一路增长而数字没跟上。数字一旦失准，整段背景叙述的可信度
就跟着一起下滑——下一个人会以为「反正才 15 处」，从而低估这些被忽略的
测试意味着多少覆盖缺口。所以凡是需要写进注释的统计数字，都应当是跑出来
的，不该是估的。

本审计只接受「外部前置条件」作为忽略理由：网络、凭据、外部工具链、
需要运行中的服务、已声明的能力缺口。裸 `#[ignore]`（无理由）与上述之外的
理由一律报出。

用法：
    python3 scripts/audit_ignored.py
    python3 scripts/audit_ignored.py --json
退出码：有问题时为 1，否则为 0。
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
from collections import Counter

ROOT = pathlib.Path(__file__).resolve().parent.parent

IGNORE_RE = re.compile(
    r'#\s*\[\s*ignore\s*(?:=\s*"([^"]*)")?\s*\]'
)

# 被认可的前置条件关键词（不区分大小写）
ACCEPTED = (
    "network",            # 需要网络
    "api_key", "api key", # 需要凭据
    "env var", "env_var", # 需要环境变量
    "toolchain", "installed",  # 需要外部工具链
    "running", "requires a running",  # 需要运行中的服务
    "mock llm",           # 已声明的能力缺口（测试依赖尚未实现的夹具）
    "sandbox", "docker",  # 需要特定运行环境
    # ── 2026-10-04 补：显式点名外部可执行文件 ──
    # 原列表只有 "toolchain"/"installed" 两个泛化词，于是
    # `requires real python3 + pytest`、`requires real mvn + jdk` 这类
    # **完全合法且信息更具体**的理由反而被判非法，导致 audit-sources 失败。
    # 判据应该是「有没有说清缺什么」，而不是「有没有用某个固定词」。
    "requires real",
    "python3", "pytest", "mvn", "maven", "go toolchain", "node", "npm",
    "javac", "jdk", "gradle", "vitest",
    "downloads plugins",  # maven 首次拉插件/依赖
    # ── 2026-10-04 补：测量型用例（本就不该在 CI 断言）──
    # bridge/tests/hook_overhead.rs 的耗时基准：绝对值随机器而变，
    # 任何硬编码门限都会在别的 runner 上变成 flaky。它是合法的。
    "耗时基准", "benchmark", "bench ",
)

# 扫描范围：自动发现，见下方 _scan_dirs()。
def _scan_dirs() -> list[pathlib.Path]:
    """自动发现所有含 Rust 源文件的顶层目录。

    与 audit_test_global_state.py / audit_orphans.py 的 _source_dirs() 保持
    同一判据：顶层目录直接含 .rs，或其某个直接子目录下含 .rs。

    曾经硬编码 ("crates", "webui", "evals")，与另外两个门禁同样的漏洞：
    ide/（4 个 crate）与 tools/ 完全没被扫。三个门禁各自漏各自的目录，
    看起来「都有审计」，合起来却仍有整片盲区。

    实测当时全仓 24 处 #[ignore] 全在 crates/，所以这个缺口没有造成实际
    漏报——但那是碰巧，不是设计。今天 ide/ 下若新增一个带 #[ignore] 的
    测试就没人管了。
    """
    dirs: list[pathlib.Path] = []
    for d in sorted(ROOT.iterdir()):
        if not d.is_dir() or d.name.startswith("."):
            continue
        if list(d.glob("*.rs")) or any(d.glob("*/src/**/*.rs")):
            dirs.append(d)
    return dirs


SCAN_DIRS = tuple(str(d.relative_to(ROOT)) for d in _scan_dirs())


def blank_comments(src: str) -> str:
    """把注释与字符串字面量内容替换为空格但保留换行，使行号不变。

    必须做这一步：文档里常会引用 `#[ignore = "..."]` 作为说明，
    例如解释「此前带 #[ignore] 是因为测试失败」。不剥离就会把文档
    里的示例当成真实的忽略点。

    2026-10-04 补：**Rust raw string**（`r"..."` / `r#"..."#`）。
    原实现只跳普通 `"` 字符串，于是
    `crates/sisyphus-tools-build/tests/rust_backend_test_e2e.rs` 里
    用 `r#"..."#` 包住的探针工程源码（内含模拟的 `#[ignore = "演示 ignored"]`）
    被当成该文件**真实**的忽略点，审计报 `rejected=12` 且退出码 1，
    使 `audit-sources` job 长期失败。
    同类问题已在 `.github/workflows/ci.yml` 的 toolchain-e2e job 里修正过一次，
    两处判据必须一致。

    **2026-10-07 补：字符串内容里的属性文本。**
    实测 `crates/sisyphus-tools-system/src/web.rs:498` 与 `:508`：
    ```ignore
    .expect("websearch 不应失败：本用例已显式 #[ignore]，手动运行时必须有真实网络");
    ```
    这句里的 `#[ignore]` 只是**字符串字面量内部的说明文字**，却在剥离后
    被 `IGNORE_RE` 匹配到，被判成「裸 #[ignore] 无理由」，使
    `make audit-ignored` 长期失败——而真实的两个 `#[ignore = "requires
    network"]` 反而正常通过。

    修法：扫描阶段**额外**返回字符串字面量区间 `spans`（内容区间，不含引号
    骨架）。`scan()` 用它判断一次 `#[ignore]` 匹配是否落在字面量内部，落在
    内部就跳过。

    为什么不能把字面量内容也 `_blank` 掉：属性 `#[ignore = "理由"]` 的理由
    **本身就写在字符串里**，剥掉内容等于把合法理由一并抹掉——实测会让
    24 处真实 `#[ignore]` 全部误报成「无理由」。注释可以剥、字符串不能剥。
    """
    out = list(src)
    i, n = 0, len(src)
    spans: list[tuple[int, int]] = []

    def _blank(a: int, b: int) -> None:
        """把 [a, b) 区间全部写成空格。

        原实现有一层 `if out[k] != '\\n'` 的保行号判断，实测证明它是**死代码**
        ——两处调用点的区间都不含换行：行注释传的是 `src.find('\\n', i)`（换行
        本身不在区间内），块注释传的是 `*/` 之后的位置。剩下的换行由主循环
        自己 append。所以把那层判断改成恒真，换行数仍是 3。

        既然判断永不生效，留着它只会让人误以为「这里有保行号逻辑，改动时要
        小心」，反而增加维护负担——实测中它也确实骗过了两条本想保护它的断言。
        真正需要保行号的是主循环，而 _blank 从不跨越换行，直接全写空格即可。
        """
        for k in range(a, min(b, n)):
            out[k] = ' '

    while i < n:
        c = src[i]
        # ── raw string: r"..." / r#"..."# / r##"..."## ──
        if c == 'r' and i + 1 < n and (src[i + 1] == '"' or src[i + 1] == '#'):
            j = i + 1
            hashes = 0
            while j < n and src[j] == '#':
                hashes += 1
                j += 1
            if j < n and src[j] == '"':
                term = '"' + '#' * hashes
                end = src.find(term, j + 1)
                if end == -1:
                    _blank(i, n)
                    break
                end += len(term)
                # 保留 r / 引号 / # 骨架，仅填掉内容（行号不变）
                _blank(j + 1, end - len(term))
                spans.append((j + 1, end - len(term)))
                i = end
                continue
        if c == '/' and i + 1 < n and src[i + 1] == '/':
            j = src.find('\n', i)
            j = n if j == -1 else j
            _blank(i, j)
            i = j
        elif c == '/' and i + 1 < n and src[i + 1] == '*':
            j = src.find('*/', i + 2)
            j = n if j == -1 else j + 2
            _blank(i, j)
            i = j
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == '\\':
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            # 记录字面量区间（不含引号骨架），供 IGNORE_RE 判定「这次匹配是否
            # 落在字符串内部」。**不能**像注释那样直接 _blank：属性
            # #[ignore = "理由"] 的理由本身就在字符串里，剥掉就等于把合法理由
            # 一并抹掉，24 处真实 ignore 会全部误报。
            spans.append((i + 1, j - 1))
            i = j
        else:
            i += 1
    return ''.join(out), spans


def next_ident(src: str, pos: int) -> str:
    """取 #[ignore] 之后最近的函数名，用于定位。"""
    m = re.search(r'fn\s+([A-Za-z_][A-Za-z0-9_]*)', src[pos:pos + 400])
    return m.group(1) if m else "<未知>"


def _real_ignores(src: str, spans: list[tuple[int, int]]) -> list[re.Match]:
    """返回**真实**的 #[ignore] 匹配，排除落在字符串字面量内的那些。

    单独抽出来是为了让 _self_check() 能直接检验这条过滤：实测把 scan() 里的
    这段判断改成恒假，真实扫描从 24 处涨到 26 处（web.rs 里两处 .expect
    字符串中的 #[ignore] 被当成真属性），而当时的自检完全没报警——因为
    它只测 blank_comments 的输出，没测 scan 真正用的这个过滤。
    """
    return [
        m for m in IGNORE_RE.finditer(src)
        if not any(a <= m.start() < b for a, b in spans)
    ]


def scan() -> list[dict]:
    findings: list[dict] = []
    for base in SCAN_DIRS:
        root = ROOT / base
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.rs")):
            try:
                raw = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            src, spans = blank_comments(raw)
            for m in _real_ignores(src, spans):
                reason = (m.group(1) or "").strip()
                line = src.count('\n', 0, m.start()) + 1
                ok = bool(reason) and any(k in reason.lower() for k in ACCEPTED)
                findings.append({
                    "file": str(path.relative_to(ROOT)),
                    "line": line,
                    "fn": next_ident(src, m.end()),
                    "reason": reason,
                    "accepted": ok,
                })
    return findings


def _self_check() -> list[str]:
    """验证剥离逻辑，返回失败说明（空列表 = 全部通过）。

    钉住的是两条来之不易的修复，都是「audit-sources job 长期失败」的根因：
      1. Rust raw string（r"..." / r#"..."#）里的 #[ignore] 是探针工程的
         源码文本，不是真实忽略点；
      2. 普通字符串字面量里的 #[ignore] 只是说明文字。

    这两条一旦退化，后果不是「漏报」而是「误报到 CI 长期红」——所以必须钉
    住，否则下一个人重构 blank_comments 时极可能把它们弄丢。
    """
    failures: list[str] = []
    cases = [
        # (输入, 期望匹配到的 #[ignore] 处数)
        ('let s = r#"#[ignore = "演示 ignored"]"#;', 0),
        ("let s = r##\"#[ignore]\"##;", 0),
        ('let s = "本用例已显式 #[ignore]，需网络";', 0),
        ('#[ignore = "requires network"]', 1),
        ('#[ignore]', 1),
        ('// #[ignore] 注释里的', 0),
        ('/* #[ignore] 块注释里的 */', 0),
        # 含转义引号的字面量：\" 结束不了字面量，里面的 #[ignore] 仍属内部
        (r'let s = "他说\" #[ignore] 算了";', 0),
        # 字面量里紧跟一个**真实**属性。若结束引号不被识别，j 会一路走到
        # 末尾、把后面所有内容都算进 span，这个真实属性就会被当成字面量
        # 内部而漏掉——实测去掉结束引号判断后，旧自检仍然全绿。
        ('let s = "含 #[ignore] 文字";\n#[ignore = "requires network"]\nfn t() {}', 1),
    ]
    for src, want in cases:
        blanked, spans = blank_comments(src)
        got = len(_real_ignores(blanked, spans))
        if got != want:
            failures.append(f"剥离+过滤对 {src!r} 得到 {got} 处，期望 {want} 处")

    # 理由文本本身写在字符串里，剥掉内容等于把合法理由一并抹掉
    src = '#[ignore = "requires network"]\nfn t() {}'
    blanked, _ = blank_comments(src)
    if 'requires network' not in blanked:
        failures.append("字符串内容被抹掉了——合法理由会全部误报成「无理由」")

    # 只断言最终计数是不够的。实测把 raw string 分支短路掉，计数仍是 0
    # ——因为 r 后的 " 照样被当普通字符串跳过，结果碰巧一致。这条断言检查
    # 的是**分隔符本身**是否被完整跳过：raw string 必须整体被当作一个字面量，
    # 其内部内容既不参与匹配也不被计入 spans。
    raw = 'let s = r#"#[ignore = "演示 ignored"]"#;'
    blanked, spans = blank_comments(raw)
    if raw.count('"') and blanked.count('"') != 2:
        failures.append(
            "raw string 的引号分隔符被当成普通字符串处理了"
            "——虽然计数碰巧正确，但 r#\"..\"# 的整体语义已丢失")
    if not spans:
        failures.append("raw string 未产生字符串区间 spans——字面量内容会被误当属性")

    # 实测过第二个漏网之鱼：把 scan() 里的 spans 过滤改成恒假，真实扫描从
    # 24 处涨到 26 处（web.rs 里两处 .expect 字符串中的 #[ignore] 被当成真
    # 属性），而当时的自检全绿——它只测 blank_comments 的输出，没测 scan
    # 真正使用的那道过滤。这里直接检验 _real_ignores（scan 调用的同一个函数）。
    lit = 'x.expect("本用例已显式 #[ignore]，需网络");'
    lit_blanked, lit_spans = blank_comments(lit)
    leaked = _real_ignores(lit_blanked, lit_spans)
    if leaked:
        failures.append(
            f"_real_ignores 未排除字符串字面量内的 #[ignore]（漏出 {len(leaked)} 处）"
            "——scan() 会把它当成真实忽略点（实测误报 24 → 26 处）")
    # 反向：该报的必须报出来，否则就成了漏报
    real = '#[ignore = "requires network"]\nfn t() {}'
    real_blanked, real_spans = blank_comments(real)
    if len(_real_ignores(real_blanked, real_spans)) != 1:
        failures.append("_real_ignores 漏掉了真实的 #[ignore] 属性——会造成漏报")

    # 行号保真。实测发现 _blank 里的「跳过换行」判断其实是死代码：主循环
    # 遇到 '\n' 时走自己的分支直接 append，换行根本不经过 _blank。所以把
    # 那句判断改成恒真，换行数仍是 3 —— 一条测不出任何东西的断言比没有更
    # 糟，它只会给人虚假的安全感。这里改为断言真实契约：剥离后换行位置不变。
    multiline = 'fn a() {\n    // 注释里的 #[ignore]\n    let x = 1;\n}'
    blanked, _ = blank_comments(multiline)
    if [i for i, c in enumerate(multiline) if c == '\n'] != \
       [i for i, c in enumerate(blanked) if c == '\n']:
        failures.append("剥离后换行位置变了——scan() 报出的行号会错位")
    if _real_ignores(blanked, blank_comments(multiline)[1]):
        failures.append("注释里的 #[ignore] 未被剥离——会被当成真实忽略点")

    return failures


def main() -> int:
    self_failures = _self_check()
    if self_failures:
        print("❌ 判据自检失败：", file=sys.stderr)
        for f in self_failures:
            print(f"  - {f}", file=sys.stderr)
        return 2

    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    items = scan()
    bad = [i for i in items if not i["accepted"]]

    if args.json:
        print(json.dumps({"ignored": items, "rejected": len(bad)},
                         ensure_ascii=False, indent=2))
        return 1 if bad else 0

    if not items:
        print("✅ 未发现 #[ignore] 测试")
        return 0

    reasons = Counter(i["reason"] or "<无理由>" for i in items)
    print(f"共 {len(items)} 处 #[ignore]：")
    for reason, count in reasons.most_common():
        print(f"  {count:>2} × {reason}")

    if not bad:
        print("\n✅ 全部忽略理由均指向外部前置条件")
        return 0

    print(f"\n❌ {len(bad)} 处忽略理由未指向外部前置条件：\n")
    for i in bad:
        print(f"  {i['file']}:{i['line']}  {i['fn']}")
        print(f"      理由：{i['reason'] or '<无理由>'}")
    print("\n被忽略的测试等于没有覆盖——CI 不会执行它们。")
    print("若原因是「测试当前失败」，应先修复测试或删除它，而不是加 #[ignore]。")
    print("可接受的理由需指向：网络、凭据/环境变量、外部工具链、需要运行中的服务，")
    print("或明确声明的能力缺口。确需专用 runner 时，请在理由中写明它依赖什么。")
    return 1


if __name__ == "__main__":
    sys.exit(main())
