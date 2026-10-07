#!/usr/bin/env python3
"""审计测试中的进程级全局状态依赖。

背景：本仓库已因此类缺陷付出两次实价——

1. `evals` bench 用 `set_current_dir` + 全局互斥锁串行化。只加锁不
   治本：若 `chdir` 之后任一断言 panic，锁的守卫析构仍会跑，但 cwd
   已经残留在一个**即将被删除**的临时目录里，此后该测试二进制内
   所有相对路径操作一并失败。已改为显式传目录参数（42e0c1e）。
2. `sisyphus-tools-scm` 的 `write_workflow` 硬编码相对路径，测试靠
   chdir 隔离。变异验证时真的把文件写进了源码树
   （`crates/sisyphus-tools-scm/.github/workflows/`）。已加显式
   `repo_root` 参数（ab42b75）。

这类缺陷比「孤儿源文件」隐蔽得多：孤儿文件至少不会绿，而 cwd 竞争
只在并行下偶发，且失败现场常被后续测试覆盖，看起来像「网络抖动」。

本审计只判定两件可机械确认的事，不去猜「这个测试看起来不并行」：

1. **无条件禁止** `set_current_dir` / `set_global_default`。这两者没有
   安全的局部替代：`set_current_dir` 改的是整个进程的 cwd，
   `set_global_default` 的全局槽位只能成功设置一次。本仓库已分别
   用显式参数和 `with_default`（线程局部）取代。
2. **要求 `set_var` 的配对清理具备 panic 安全性**。若清理语句出现在
   同一函数体内、且位于至少一个 `assert`/`panic!`/`unwrap`/`expect`
   之后，则断言失败时清理被跳过，变量**永久残留**给同二进制的后续
   测试。这类残留的表现是「某个无关测试突然开始认证失败」。

刻意不判定的（避免门禁变成噪音而被弃用）：
- `set_var` 出现在**生产代码**里：那是配置覆盖机制本身，不是不健全。
- 纯串行测试二进制（`tests/` 下每个文件独立进程）里的 `set_var`。
- 互斥锁/串行化标注：它只是缓解，不是安全性的证明。

用法：
    python3 scripts/audit_test_global_state.py
    python3 scripts/audit_test_global_state.py --json
退出码：有问题时为 1，否则为 0。
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# 无条件禁止：进程级、且没有局部替代。
FORBIDDEN = {
    "set_current_dir": (
        "改的是整个进程的 cwd，并行测试互踩；若中途 panic 还会残留在已删除目录。"
        "请改为显式传入根目录参数（参考 write_workflow(repo_root)）。"
    ),
    "set_global_default": (
        "进程全局槽位只能成功设置一次，与其他测试争抢。"
        "请改用 tracing::subscriber::with_default（线程局部）。"
    ),
}

SET_VAR_RE = re.compile(r"(?:std::)?env::set_var\s*\(\s*")
REMOVE_VAR_RE = re.compile(r"(?:std::)?env::remove_var\s*\(")

# 会让「顺序执行」的假设失效的语句。
BARRIER_RE = re.compile(
    r"\bassert"
    r"|\bassert_eq!\b|\bassert_ne!\b|\bassert_matches!\b"
    r"|\bpanic!\b|\bunreachable!\b|\btodo!\b"
    r"|\.unwrap\(\)|\.expect\("
)

SCAN_DIRS = ("crates", "webui", "evals")

# 生产配置的真实变量名。测试若直接改写它们，风险远高于改写
# 测试专用名：这类变量的语义是「全局开关」，改它等于改整个二进制的
# 行为，且 panic 后残留会静默影响后续所有测试。
#
# 实测案例：webui 的三个路由测试直接 set/remove `WEBUI_API_KEY`
# （生产变量名），彼此的正确性只靠一把互斥锁的调度顺序维持。
PRODUCTION_ENV_VARS = {
    "WEBUI_API_KEY",
    "SISYPHUS_COGNITIVE_LOOP_MAX_ITERATIONS",
    "GOOGLE_GENERATIVE_AI_API_KEY",
}

# 抗 panic 的清理形态：出现其一即视为已用 RAII 兜底。
RAII_GUARD_RE = re.compile(r"\bimpl\s+Drop\b|\bstruct\s+\w*Guard\b|\bDropGuard\b")

# 守卫的创建点：`EnvVarGuard::set(K, …)` / `unset(K)`。
#
# 关键：必须**按变量名**判定，不能只看「函数体内有没有守卫」。
# 守卫只覆盖它自己那个名字：
#   let _g = EnvVarGuard::unset("VAR_A");
#   env::set_var("VAR_B", "x");   // ← VAR_B 没人兜底，仍是泄漏
# 我最初写成「有守卫即全函数豁免」，变异注入一个新变量名后门禁毫无反应，
# 等于把门禁关掉了。放宽判据必须同时证明它仍能抓到真缺陷。
GUARD_CONSTRUCT_RE = re.compile(
    r"\bEnvVarGuard\s*::\s*(?:set|unset|set_owned|unset_owned)\s*\(\s*"
    r'(?:&?\s*)?"?([A-Za-z_][A-Za-z0-9_]*)'
)

# 判定「是否改写生产变量名」需要读出字符串字面量内容，故对源码多取一份
# 保留字符串的副本。代价是 raw string 里的内容也可能被误当作变量名，
# 故仅当该变量名确实在 PRODUCTION_ENV_VARS 白名单里时才报出。
KEEP_STRINGS = True


def blank_noncode(src: str, keep_strings: bool = False) -> str:
    """把注释与字符串字面量内容替换为空格但保留换行，使行号不变。

    与 audit_ignored.py 同法：文档和注释里常引用 `std::env::set_var`
    作为反例说明，不剥离就会把说明当成真实用法。

    `keep_strings=True` 时**保留**字符串字面量的内容（连同引号），只剥离
    注释。原因：判定「是否改写了生产变量名」必须读出 `set_var("WEBUI_API_KEY")`
    里的变量名，而变量名恰好就是字符串内容；剥离后该判定会静默失效
    ——门禁看似在跑，实际永远报不出这一类。
    """
    out = list(src)
    i, n = 0, len(src)

    def _blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if c == "r" and i + 1 < n and (src[i + 1] == '"' or src[i + 1] == "#"):
            j = i + 1
            hashes = 0
            while j < n and src[j] == "#":
                hashes += 1
                j += 1
            if j < n and src[j] == '"':
                term = '"' + "#" * hashes
                end = src.find(term, j + 1)
                if end == -1:
                    _blank(i, n)
                    break
                end += len(term)
                if not keep_strings:
                    _blank(j + 1, end - len(term))
                i = end
                continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j == -1 else j
            _blank(i, j)
            i = j
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            j = src.find("*/", i + 2)
            j = n if j == -1 else j + 2
            _blank(i, j)
            i = j
        elif c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            if not keep_strings:
                _blank(i + 1, j - 1)
            i = j
        else:
            i += 1
    return "".join(out)


def enclosing_fn(src: str, pos: int) -> tuple[str, int]:
    """返回 (函数名, 函数体起始偏移)。找不到返回 ("<文件顶层>", 0)。"""
    head = src[:pos]
    m = None
    for m in re.finditer(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)", head):
        pass
    if m is None:
        return "<文件顶层>", 0
    # 函数体起点：从 fn 后的第一个 '{' 起
    brace = src.find("{", m.end())
    if brace == -1 or brace > pos:
        return m.group(1), m.start()
    # 粗略的花括号配平，找到本函数的闭合位置
    depth = 0
    for k in range(brace, len(src)):
        if src[k] == "{":
            depth += 1
        elif src[k] == "}":
            depth -= 1
            if depth == 0:
                return m.group(1), brace
    return m.group(1), brace


def env_var_name(src: str, pos: int) -> str | None:
    """取 `set_var(...)` 第一个参数里的变量名。

    变量名总是以字符串字面量给出，在保留字符串的副本上需要先剥掉
    开头的引号。解析不出时返回 None（调用方按「未受保护」处理，
    宁可误报也不漏报）。
    """
    rest = src[pos:].lstrip()
    if rest.startswith('"'):
        rest = rest[1:]
    m = re.match(r"[A-Za-z_][A-Za-z0-9_]*", rest)
    return m.group(0) if m else None


def check_panic_safe_cleanup(src: str, rel: str) -> list[dict]:
    """判定 2：set_var 之后，清理是否会被 panic 跳过。

    两种缺陷形态都算：
    - `set_var` 之后完全没有同函数内的 `remove_var`（最常见）
    - 有 `remove_var`，但位于 assert/unwrap 之后，断言失败即被跳过
    """
    out: list[dict] = []
    for m in SET_VAR_RE.finditer(src):
        fn, body_start = enclosing_fn(src, m.start())
        line = src.count("\n", 0, m.start()) + 1
        body = src[body_start:]

        # 该 set_var 的变量名若被同函数内的守卫覆盖，则已被 panic
        # 安全地兜住。注意必须比对**变量名**，不能只看有无守卫。
        var_name = env_var_name(src, m.end())
        guarded = var_name is not None and var_name in {
            g for g in GUARD_CONSTRUCT_RE.findall(body) if g
        }
        if guarded:
            continue

        removes = [
            r for r in REMOVE_VAR_RE.finditer(src)
            if enclosing_fn(src, r.start())[0] == fn
        ]
        # 任一 remove 出现在 barrier 之前 → 清理早于断言风险，安全
        protected = False
        for r in removes:
            r_body = src[body_start:]
            idx = r_body.find(r.group(0))
            if idx != -1 and not BARRIER_RE.search(r_body[:idx]):
                protected = True
                break

        if not protected:
            detail = (
                "同函数内无清理语句，变量永久残留给同二进制的后续测试"
                if not removes else
                "清理语句位于 assert/panic/unwrap 之后，断言失败即被跳过，变量永久残留"
            )
            out.append({
                "file": rel, "line": line, "fn": fn,
                "kind": "set_var_without_panic_safe_cleanup",
                "detail": detail, "accepted": False,
            })
    return out


def check_production_env_vars(src: str, rel: str) -> list[dict]:
    """判定 3：测试直接改写生产配置的真实变量名。

    这类比改写测试专用名危险得多：变量语义是「全局开关」，
    改它等于改整个二进制的行为，且 panic 后残留会静默影响后续
    所有测试——失败现场看起来像「某个无关测试突然认证失败」。
    """
    out: list[dict] = []
    for m in SET_VAR_RE.finditer(src):
        name = env_var_name(src, m.end())
        if not name:
            continue
        if name not in PRODUCTION_ENV_VARS:
            continue
        fn, _ = enclosing_fn(src, m.start())
        line = src.count("\n", 0, m.start()) + 1
        out.append({
            "file": rel, "line": line, "fn": fn,
            "kind": "mutates_production_env_var",
            "detail": (
                f"{name} 是生产配置的真实变量名，不是测试专用名。"
                "测试改写它等于改整个二进制的行为，且 panic 后残留会静默影响后续测试。"
                "请改为向被测代码注入显式配置参数。"
            ),
            "accepted": False,
        })
    return out


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
            src = blank_noncode(raw)
            # 变量名判定需要读出字符串内容，故另取一份保留字符串的副本。
            src_kept = blank_noncode(raw, keep_strings=True) if KEEP_STRINGS else src
            rel = str(path.relative_to(ROOT))

            # ── 1. 无条件禁止项 ──
            for name, why in FORBIDDEN.items():
                for m in re.finditer(r"(?:::|\.)" + name + r"\s*\(", src):
                    line = src.count("\n", 0, m.start()) + 1
                    fn, _ = enclosing_fn(src, m.start())
                    findings.append({
                        "file": rel, "line": line, "fn": fn,
                        "kind": name, "detail": why, "accepted": False,
                    })

            findings.extend(check_production_env_vars(src_kept, rel))

            # ── 2/3：已有 RAII 守卫的文件整段跳过 panic 判定，
            # 守卫的 Drop 会跑，天然抗 panic。生产变量判定仍要跑——
            # 守卫只能保证「恢复原值」，不能保证「不改生产语义」。
            if not RAII_GUARD_RE.search(src):
                # 须传保留字符串的副本：判定「set_var 的变量名是否被
                # 同函数内的守卫覆盖」要读出字符串内容，用剥离版会恒为
                # False，守卫全被当成不存在。
                findings.extend(check_panic_safe_cleanup(src_kept, rel))
    return findings


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    items = scan()
    bad = [i for i in items if not i["accepted"]]

    if args.json:
        print(json.dumps({"findings": items, "rejected": len(bad)},
                         ensure_ascii=False, indent=2))
        return 1 if bad else 0

    if not items:
        print("✅ 未发现依赖进程级全局状态的测试")
        return 0

    print(f"共 {len(items)} 处进程级全局状态依赖：\n")
    for i in bad:
        print(f"  {i['file']}:{i['line']}  {i['fn']}()")
        print(f"      {i['kind']}：{i['detail']}")
    print("\n并行测试下这些写法会互相污染，且 panic 会让清理被跳过。")
    print("请改为显式传参（cwd）或线程局部状态；清理请用能抗 panic 的作用域守卫。")
    return 1


if __name__ == "__main__":
    sys.exit(main())
