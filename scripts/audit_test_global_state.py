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

# 守卫自身的实现文件。这里的 `set_var` 不是「测试里忘记清理」，
# 而是**实现本身**——恢复动作就在同文件的 `impl Drop` 里。
# 按定义不能要求它使用自己。
#
# 豁免范围是**行级**：仅 `#[cfg(test)]` 之前的实现区，见
# check_panic_safe_cleanup 的 before_line 参数。
#
# 为什么必须缩到行级：早期实现是**文件级**豁免，理由是「按函数区分
# 实现代码与测试代码需要知道边界」。但这个边界是可以机械判定的——
# `#[cfg(test)]` 就是测试区的起点，rustfmt 保证它顶格。实测
# env_guard.rs 的 7 处裸调用全部落在 `#[cfg(test)]`（第 195 行）
# 之前，测试区本就没有裸调用。于是文件级豁免多出来的那一大段
# （整个 mod tests）是一个**永不触发的保护**，却在测试区真的引入裸
# set_var 时完全沉默——正是我们要防的那类漏报。
# 上述文件的实现区边界：首个 `#[cfg(test)]` 所在行（1-based）。
# 若该文件不再含测试区，自检会失败，强制重新审视豁免。
#
# 允许**滞后**（actual > limit）但不允许超前（actual < limit）：
# 超前意味看豁免范围比真实测试区还宽，会盖掉本该报出的泄漏；
# 滞后只会让更多实现代码被检查（多报噪音，不漏报）。这个方向上的
# 不对称是刻意的：门禁宁可吵不可静默。
GUARD_IMPL_LINE_LIMITS = {
    "crates/sisyphus-types/src/env_guard.rs": 195,
}


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

# 注：原先这里有个 RAII_GUARD_RE（「文件里出现过 impl Drop / *Guard
# 即整文件免检」）。已删除——那等于把门禁关掉：impl Drop 只对它自己
# 那枚守卫变量生效，同文件里另写的裸 set_var 与它毫无关系。实测
# crates/sisyphus-tools-llm/src/http_client.rs 因此让 9 处真实泄漏
# 全部免检。现在改为按「变量名 + 作用域覆盖」逐点判定。

# 守卫的创建点：`EnvVarGuard::set(K, …)` / `unset(K)`。
#
# 关键：必须**按变量名**判定，不能只看「函数体内有没有守卫」。
# 守卫只覆盖它自己那个名字：
#   let _g = EnvVarGuard::unset("VAR_A");
#   env::set_var("VAR_B", "x");   // ← VAR_B 没人兜底，仍是泄漏
# 我最初写成「有守卫即全函数豁免」，变异注入一个新变量名后门禁毫无反应，
# 等于把门禁关掉了。放宽判据必须同时证明它仍能抓到真缺陷。
#
# 引号写成可选的 `"?…"` 是安全的：正则引擎在后续 `([A-Za-z_]…)` 匹配
# 失败时会回溯，不消耗那个可选引号，故 group(1) 仍是干净的变量名。
# （我一度怀疑这里把引号吞进了捕获组导致守卫全部失效，实测证伪：
# group(1) 正确返回 K。那次真正的漏报根源是下面的整文件跳过。）
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


def _in_same_block(body: str, a: int, b: int) -> bool:
    """判断 a、b 两个偏移是否落在**同一对花括号**内。

    用于判定「守卫的作用域是否覆盖本次 set_var」：guard 靠词法作用域
    生效，所以「覆盖」等价于「在同一对花括号内」。

    做法：取二者中较早者，向前找到它所在块的开括号；再从该开括号
    正向配平找到闭括号；判断较晚者是否落在这一区间内。

    已知局限（保守方向：宁可多报不漏报）：字符串字面量里的花括号
    会干扰配平。调用方传入的 `body` 已剥离注释但保留字符串，故含
    `{` 的字面量（如 `"{}"`）可能造成误判。误判后果是「本该豁免的
    被报出来」，属噪音而非漏报，是可接受的方向。
    """
    lo, hi = (a, b) if a <= b else (b, a)
    # 从 lo 往前找最近的未配平开括号，即 lo 所在块的开头
    depth = 0
    open_at = -1
    for k in range(lo - 1, -1, -1):
        ch = body[k]
        if ch == "}":
            depth += 1
        elif ch == "{":
            if depth == 0:
                open_at = k
                break
            depth -= 1
    if open_at == -1:
        # lo 之前没有更外层的花括号，说明它就在函数体根层
        return True
    # 从 open_at 正向配平，找该块的闭括号
    depth = 0
    for k in range(open_at, len(body)):
        ch = body[k]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return open_at <= hi <= k
    # 未配平（源码被截断等），保守视为不在同一块
    return False


def check_panic_safe_cleanup(
    src: str, rel: str, before_line: int | None = None
) -> list[dict]:
    """判定 2：set_var 之后，清理是否会被 panic 跳过。

    两种缺陷形态都算：
    - `set_var` 之后完全没有同函数内的 `remove_var`（最常见）
    - 有 `remove_var`，但位于 assert/unwrap 之后，断言失败即被跳过

    `before_line` 为 1-based 行号下限：位于该行**之前**的调用不检查，
    用于守卫自身实现文件的豁免（见 GUARD_IMPL_LINE_LIMITS）。

    比较方向写反过一次：`line >= before_line` 就跳过，等于豁免该行
    **之后**的全部调用，范围整个倒过来——守卫的实现区被当成测试检查，
    而真正要防的 mod tests 反而被放过。那次是「碰巧」报出了 2 条噪音
    才没静默溜过去；若守卫实现本身恰好满足其它判据，就会静默失效。
    故 `before_line` 的语义由 _self_check() 单独钉住。
    """
    out: list[dict] = []
    for m in SET_VAR_RE.finditer(src):
        line = src.count("\n", 0, m.start()) + 1
        if before_line is not None and line < before_line:
            continue
        fn, body_start = enclosing_fn(src, m.start())
        body = src[body_start:]

        # 该 set_var 的变量名若被同函数内的守卫覆盖，则已被 panic
        # 安全地兜住。注意必须比对**变量名**，不能只看有无守卫。
        #
        # 2026-10-07 修正：这里曾造成真实漏报。`env_guard.rs` 自己的
        # 测试里，守卫（`EnvVarGuard::set(KEY, …)`）与裸 `set_var(KEY, …)`
        # **同时存在**——守卫管的是它自己那条路径，裸 set_var 是**绕过**
        # 守卫的另一条路径，panic 后照样泄漏。原判据只看「同函数内有没有
        # 这个变量名的守卫」，于是把绕过路径一并豁免了。
        #
        # 判据收紧为：**该 set_var 必须本身就处在守卫的作用域内**——
        # 即它出现在守卫创建语句的**之前**（守卫创建时会把 previous 记成
        # 此刻的值，故它之前的所有改动都会被一并恢复）。守卫创建之后的
        # 裸 set_var 是绕过，不豁免。
        var_name = env_var_name(src, m.end())
        guarded = False
        if var_name is not None:
            # 判定的是「本次 set_var 的效果会不会被某枚守卫的 Drop 撤销」。
            #
            # 语义推演（守卫与 set_var 的三种相对位置）：
            #   A) 守卫在外层、set_var 在其作用域内：守卫析构时恢复自己
            #      的 previous，覆盖掉 set_var ⇒ **安全**，豁免。
            #   B) 守卫的块已结束、set_var 在其之外（典型：
            #      `{ let _g = Guard::set(K, "true"); … }` 之后又
            #      `set_var(K, "false")`）：守卫早已析构，set_var
            #      无任何兜底 ⇒ **泄漏**，必须报出。
            #   C) set_var 在守卫创建之前：守卫 previous 记成 set_var
            #      设下的值，析构恢复成同一个值 ⇒ 无害，但这个写法
            #      本身可疑（set_var 的意图落空），仍按泄漏报出更安全。
            #
            # 我最初写成「函数内有守卫即豁免」，漏掉了 B；随后改成
            # 「守卫在 set_var 之后才豁免」，把 A 也漏了（方向相反，
            # 恰好是 B 的反面）。两者都不是作用域判定。
            #
            # 正确做法是花括号配平：找出守卫创建语句所在的块，判断
            # 本次 set_var 的偏移是否落在该块内。guard 在 Rust 里靠
            # 词法作用域生效，「作用域覆盖」就是「在同一对花括号内」，
            # 而配平是机械可判定的。
            guarded = any(
                _in_same_block(body, g.start(), m.start() - body_start)
                for g in GUARD_CONSTRUCT_RE.finditer(body)
                if g.group(1) == var_name
            )
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


def _self_check() -> list[str]:
    """判据自检：用六个合成用例验证豁免逻辑既有判别力又不误伤。

    这个门禁的判据改过两次，两次都漏掉了真实缺陷：
      1. 「函数内有守卫即豁免」 → 漏掉不同变量名，以及守卫块已结束的情形
      2. 「文件里有 impl Drop 即整文件免检」→ 让 http_client.rs 的
         9 处真实泄漏、env_guard.rs 自身测试区的裸调用全部免检

    两次都没有症状：脚本照常运行、输出「✅ 未发现问题」。只有拿已知
    该报的输入去问「你报不报」，才能发现。所以固化成自检。

    返回未通过的用例描述；空列表表示全部符合预期。
    """
    cases: list[tuple[str, str, bool]] = [
        ("守卫在外层、set_var 在其内", '''
fn t() {
    let _g = EnvVarGuard::set("K", "y");
    unsafe { env::set_var("K", "x") };
    assert!(true);
}''', False),
        ("守卫的块已结束、set_var 在其外", '''
fn t() {
    {
        let _g = EnvVarGuard::set("K", "y");
        assert!(true);
    }
    unsafe { env::set_var("K", "x") };
    assert!(true);
}''', True),
        ("无守卫、清理晚于断言", '''
fn t() {
    unsafe { env::set_var("K", "x") };
    assert!(true);
    unsafe { env::remove_var("K") };
}''', True),
        ("守卫管的是别的变量名", '''
fn t() {
    let _g = EnvVarGuard::set("OTHER", "y");
    unsafe { env::set_var("K", "x") };
    assert!(true);
}''', True),
        ("守卫在前、set_var 后、同作用域", '''
fn t() {
    let _g = EnvVarGuard::set("K", "y");
    unsafe { env::set_var("K", "x") };
}''', False),
        ("守卫与 set_var 同在内层块", '''
fn t() {
    {
        let _g = EnvVarGuard::set("K", "y");
        unsafe { env::set_var("K", "x") };
    }
}''', False),
    ]
    failures: list[str] = []

    # before_line 的方向（“该行之前不检查”）必须被钉住：这个参数一
    # 旦写反，豁免范围就整个倒过来，而且**症状是噪音而非报错**——
    # 守卫实现区会被当成测试报出两条，看着像真实发现，很容易被直接
    # 修代码而不是修判据；反之若守卫实现碰巧满足其它判据，则静默
    # 失效。两边都是“看起来在跑”。
    #
    # 样本的 set_var 在第 4 行（行号从下方字面量里实测，不写死，
    # 避免改排版后失配）。语义是「**行号小于** before_line 即豁免」，
    # 故分界在 set_var 上方时豁免，分界在下方时仍需检查：
    #   before_line=3 → 4<3 假 → set_var 被检查（报出）
    #   before_line=4 → 4<4 假 → 分界正好落在它身上，被检查（报出）
    #   before_line=5 → 4<5 真 → 落入豁免区
    # 三个值一起测：只测一个，分界两侧的效果会互相抵消，
    # 方向写反也看起来照样通过。
    #
    # 我最初把期望写成「分界在上方→豁免」，与实现相反，于是自检
    # 报我三条全错。实测后确认是**期望**写错、判据本身是对的：
    # 豁免整个测试区正是要防的漏报面，故必须是「行号小于分界才豁免」。
    dir_src = (
        "struct G;\nimpl Drop for G { fn drop(&mut self) {} }\n"
        "fn t() {\n"
        "    unsafe { env::set_var(\"K\", \"x\") };\n"
        "    assert!(true);\n"
        "}\n"
    )
    sv_line = dir_src.count("\n", 0, dir_src.index("set_var")) + 1
    for limit, want_reported in (
        (sv_line - 1, True),   # 分界在 set_var 上方 → 它在豁免区之外
        (sv_line, True),       # 分界正好落在它身上 → 行号相等，不豁免
        (sv_line + 1, False),  # 分界在断言行 → set_var 落入豁免区
    ):
        got = bool(check_panic_safe_cleanup(
            dir_src, "<self-check>", before_line=limit))
        if got != want_reported:
            failures.append(
                f"before_line={limit}（set_var 在第 {sv_line} 行）："
                f"期望{'报出' if want_reported else '豁免'}，"
                f"实际{'报出' if got else '豁免'}——方向可能写反"
            )

    for name, src, want_reported in cases:
        got = bool(check_panic_safe_cleanup(
            blank_noncode(src, keep_strings=True), "<self-check>"))
        if got != want_reported:
            failures.append(
                f"{name}：期望{'报出' if want_reported else '豁免'}，实际{'报出' if got else '豁免'}"
            )

    # 上一组只测 check_panic_safe_cleanup 这个纯函数，覆盖不到
    # scan() 层的「整文件跳过」——那正是漏掉 http_client.rs 9 处的
    # 那一行。若自检不经过 scan()，把整文件跳过加回来它照样全绿。
    #
    # 必须**端到端**跑一次 scan()：在临时目录里造一个「含 impl Drop
    # 但有裸 set_var」的文件，看 scan() 是否报出它。之前试过在脚本内
    # 拿合成源码直接判断，条件恒真（样本自己就含 impl Drop），
    # 等于没测——这类「看起来在测、实际恒真」的断言比没有更糟。
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        root = pathlib.Path(td)
        crate = root / "crates" / "sisyphus-selfcheck-probe"
        crate.mkdir(parents=True)
        (crate / "probe.rs").write_text(
            "struct UnrelatedGuard;\n"
            "impl Drop for UnrelatedGuard { fn drop(&mut self) {} }\n"
            "fn t() {\n"
            "    unsafe { std::env::set_var(\"SELFCHECK_K\", \"x\") };\n"
            "    assert!(true);\n"
            "}\n",
            encoding="utf-8",
        )
        real_root = ROOT
        try:
            globals()["ROOT"] = root
            globals()["SCAN_DIRS"] = ("crates",)
            probe_findings = scan()
        finally:
            globals()["ROOT"] = real_root
            globals()["SCAN_DIRS"] = ("crates", "webui", "evals")
        if not probe_findings:
            failures.append(
                "含 impl Drop 的文件里的裸 set_var 未被报出——"
                "scan() 仍存在整文件跳过逻辑；impl Drop 只对它自己那枚"
                "守卫变量生效，同文件的裸 set_var 与它无关"
            )

    # 豁免边界必须是**真实**的 `#[cfg(test)]` 行，不能是随手写的数字。
    # 只能偏小（滞后），不能偏大（超前）：
    #   超前 → 豁免区伸进测试区，本该报出的泄漏被盖掉（漏报）；
    #   滞后 → 额外检查了几行实现代码，最多报噪音（不漏报）。
    # 即「宁可吵不可静默」。故只判 `actual < limit`。
    #
    # 注：滞后在本文件上恰好无害——实测实现区的裸调用在第 149 行，
    # 远早于任何合理的滞后。但这不是判据的前提，写在这里是为了
    # 避免有人把滞后当成“也没事”，进而把数字越调越大。
    for rel, limit in GUARD_IMPL_LINE_LIMITS.items():
        path = ROOT / rel
        if not path.exists():
            failures.append(f"豁免文件不存在：{rel}")
            continue
        lines = path.read_text(encoding="utf-8").splitlines()
        actual = next(
            (i for i, ln in enumerate(lines, 1)
             if ln.strip().startswith("#[cfg(test)]")),
            None,
        )
        if actual is None:
            failures.append(
                f"{rel} 已无 #[cfg(test)]，但仍在豁免列表里——"
                "豁免范围失去意义，请重新审视"
            )
        elif actual < limit:
            failures.append(
                f"{rel} 的 #[cfg(test)] 在第 {actual} 行，"
                f"早于豁免边界 {limit}——豁免范围已与实际不符"
            )

    return failures


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

            # ── 2/3：panic 安全的判定逐个 set_var 做，不再整文件跳过。
            #
            # 我曾在这里按「文件里出现过 impl Drop / *Guard 就整段跳过」
            # 放行，理由是「守卫的 Drop 会跑」。这是错的：impl Drop 只
            # 对**它自己那枚守卫变量**生效，同文件里另写的裸 set_var
            # 与它毫无关系。实测 crates/sisyphus-tools-llm/src/http_client.rs
            # 因一个无关的 impl Drop 而让 9 处 set_var 全部免检，
            # crates/sisyphus-types/src/env_guard.rs 同理。
            #
            # 守卫的豁免现在由 check_panic_safe_cleanup 内部按
            # 「变量名 + 作用域是否覆盖」逐点判定。
            #
            # 守卫**实现文件**自身是唯一例外：那里 set_var 就是实现
            # 本身，恢复动作在其 impl Drop 里，按定义不能要求它使用
            # 自己。豁免精确到 `#[cfg(test)]` 之前——文件级豁免会连带
            # 放过同文件的 mod tests，而那正是要防的漏报面。
            #
            # 须传保留字符串的副本：判定变量名要读出字符串内容，
            # 用剥离版会恒为 False，守卫全被当成不存在。
            limit = GUARD_IMPL_LINE_LIMITS.get(rel)
            findings.extend(
                check_panic_safe_cleanup(src_kept, rel, before_line=limit)
            )
    return findings


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    # 判据自检先行：判据本身失效时，下面 scan() 无论报 0 条还是 N 条
    # 都没有意义。宁可在这里失败，也不要输出一个建立在坏判据上的
    # 「✅ 未发现问题」。
    self_failures = _self_check()
    if self_failures:
        print("❌ 判据自检未通过，门禁本身已失效：\n")
        for f in self_failures:
            print(f"  - {f}")
        print("\n这意味着下面的扫描结果不可信。请先修判据。")
        return 2

    items = scan()
    bad = [i for i in items if not i["accepted"]]

    if args.json:
        print(json.dumps({"findings": items, "rejected": len(bad),
                          "self_check": "ok"},
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
