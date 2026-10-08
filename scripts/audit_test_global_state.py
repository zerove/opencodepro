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

关于「测试」与「生产」的界定，先前这里写的是「刻意不判定 `set_var`
出现在生产代码里」。实测与实现相反，且**实现是对的**：判据对生产代码
同样生效（探针：生产函数里 `set_var` 后无清理 → 报出 1 条；有配对清理
→ 0 条）。理由是生产代码里的裸 `set_var` 同样污染进程环境，只是它更
可能是「有意的配置覆盖」而非「忘了清理」；真正的配置覆盖应通过
`Config` 而非直接改环境变量。刻意豁免的只有一处：**守卫自身的实现
文件**（`env_guard.rs`），因为那里 `set_var` 就是实现本身，恢复动作在
其 `impl Drop` 里，按定义不能要求它使用自己。

刻意不判定的（避免门禁变成噪音而被弃用）：
- 互斥锁/串行化标注：它只是缓解，不是安全性的证明。
- 其他进程级状态（`static mut`、`OnceLock` 写入、子进程 `Command::env`）：
  前两者由 `no_panic_check` 与 review 约定覆盖，`Command::env` 只影响
  子进程、不污染本进程。纳入前两者会引入无法自动判定的假阳性。

上面两条「曾经写着不判定、实际却要判定」的承诺已删除：实现对**任何
位置的** `set_var` 一视同仁，含生产代码与 `tests/` 目录（实测：在
`crates/sisyphus-mem/tests/` 下放一个裸 `set_var` → 报出 1 条）。这里
保留更严的行为——`tests/` 下虽然每个文件独立进程，但裸 `set_var` 加上
断言失败即残留，仍是该改的。**当前 `tests/` 下没有任何真实 `set_var`**
（`snapshot_writeop.rs` 里的 `write_op_set_var` 只是枚举变体名，不是环境
变量写入），所以这条承诺从未被兑现过、也没造成过噪音。

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

# 扫描范围：自动发现顶层含 .rs 文件的目录，不在此硬编码。
#
# 原先是 SCAN_DIRS = ("crates", "webui", "evals")，实测漏掉了 ide/（10 个
# .rs）与 tools/（1 个）——门禁对这两个目录里的真实泄漏完全沉默。这与
# 本项目反复遇到的失效模式同构：硬编码的清单不会随仓库生长而更新，
# 且**症状是零**（脚本照常输出「✅ 未发现问题」）。用探针证实过：往
# ide/ 放一个含裸 set_var 的文件，scan() 报 0 条；把 ide 加进清单后
# 报 1 条。
#
# 改为「凡是顶层目录下有 .rs 文件就扫」，并显式排除产物/工具目录。
# 硬编码的代价是新目录一出现就静默漏扫；自动发现的代价是多扫几个
# 无害目录（最多报噪音，不漏报）——这个方向上的不对称是刻意的。
#
# 注意：不能用「git ls-files」代替——那会漏掉未提交的新文件，而新
# 文件恰恰是泄漏最常被引入的地方。
SCAN_EXCLUDE_DIRS = frozenset({
    # 编译产物与第三方代码：不是本项目的测试，不该由本门禁判定。
    "target", "node_modules", "book", "vendor", "dist", "build",
    # 基础设施目录，不含 Rust 源码。
    ".git", ".github", ".cargo", ".githooks", ".devcontainer", ".jcode",
    ".sisyphus", "deploy", "static", "docs", "scripts",
})


def _scan_dirs() -> list[str]:
    """返回顶层所有含 .rs 文件、且不在排除表里的目录名（已排序）。

    刻意不写死清单。实测 ide/ 曾因不在硬编码列表里而完全免检。
    """
    out: list[str] = []
    for child in ROOT.iterdir():
        if not child.is_dir() or child.name.startswith("."):
            continue
        if child.name in SCAN_EXCLUDE_DIRS:
            continue
        try:
            has_rs = any(child.rglob("*.rs"))
        except OSError:
            continue
        if has_rs:
            out.append(child.name)
    return sorted(out)

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
# env_guard.rs 的实现区全部裸调用（6 处）都落在 `#[cfg(test)]`
# （第 195 行）之前，测试区本就没有裸调用。于是文件级豁免多出来的那一大段
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
# 全部免检（修复前的状态；现已全部改用 EnvVarGuard，裸调用为 0）。
# 现在改为按「变量名 + 作用域覆盖」逐点判定。

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


CLOSURE_LABEL = "<闭包>"

# 这些关键字开启的是**表达式修饰块**，不引入新作用域：块内语句与
# 块外同属一个作用域。与之相对，loop/if/match/while/for/unsafe fn
# 等是块语句或函数声明，会真正开启作用域。
MODIFIER_KEYWORDS = {"unsafe", "async", "const", "move"}


def _pos_in_closure(src: str, pos: int) -> bool:
    """pos 是否落在某个闭包体内（词法上）。

    判据：块链中任一层经 _block_is_closure 判定为闭包体即为 True。

    为什么必须机械配平而不能用函数名近似：闭包体与外层函数体
    同属一个 `fn`，靠名字分不出来。而它们的**执行时机**完全不同
    ——这正是漏报的根源。
    """
    return any(_block_is_closure(src, b)
               for b in innermost_block_chain(src, pos))


def innermost_block_chain(src: str, pos: int) -> list[int]:
    """返回包含 pos 的所有**作用域**开括号偏移，由外到内。

    会跳过 `unsafe { … }` 这类**不引入新作用域**的包装：`unsafe`
    是表达式修饰符而非块语句，它里面的语句与外层同属一个作用域。
    不跳过会误判：两个连续的 `unsafe { set_var(..) }` /
    `unsafe { remove_var(..) }` 各自最内层块不同，就被当成两个作用域
    而无法配对——实测闭包内「成对设值与清理」因此被误报。
    """
    _, fn_brace = enclosing_fn(src, pos)
    scan_from = fn_brace if fn_brace <= pos else 0
    opens: list[int] = []
    # 修饰符块（`unsafe {`）**不入栈**，它的 '}' 也**不能弹栈**。
    # 入栈与弹栈必须成对：若跳过入栈却照常弹栈，每遇到一个 unsafe
    # 块就会多弹一次，把本该在栈里的闭包体、函数体一并弹掉。
    # 实测正是这样：闭包内成对的那条用例里，remove 的块链被弹成
    # [函数体]，闭包体凭空消失，_pos_in_closure 随之误判为 False。
    skip_until: int | None = None
    for k in range(scan_from, pos):
        if skip_until is not None:
            if src[k] == "{":
                skip_until += 1
            elif src[k] == "}":
                skip_until -= 1
                if skip_until == 0:
                    skip_until = None
            continue
        if src[k] == "{":
            if opens and _is_modifier_block(src, k):
                skip_until = 1  # 整个 unsafe 块当作不存在
            else:
                opens.append(k)
        elif src[k] == "}" and opens:
            opens.pop()
    return opens


def _is_modifier_block(src: str, brace: int) -> bool:
    """brace 是否是 `unsafe {` / `async {` 这类修饰符块的开括号。

    仅当它**嵌套在另一个块内**时才算修饰符（顶层 `{` 不跳）。
    `unsafe fn` 的函数体不会误判：它的 '{' 前面是 `fn name(...)`，
    取不到 MODIFIER_KEYWORDS 里的词。
    """
    i = brace - 1
    while i >= 0 and src[i] in " \t\r\n":
        i -= 1
    if i < 0:
        return False
    end = i + 1
    while i >= 0 and (src[i].isalnum() or src[i] == "_"):
        i -= 1
    return src[i + 1:end] in MODIFIER_KEYWORDS


def _block_is_closure(src: str, brace: int) -> str | None:
    """brace 是否是闭包体的 '{'。是则返回 "<闭包>"，否则 None。

    判据：紧邻 brace 之前（跳过空白）应当是闭包参数的**收尾竖线**：
    `|| {` 或 `|x: T| {`。找到那个 `|` 后再往前找配对的起始竖线，
    两者之间不得出现 `;`、`{`、`}`（这些字符不可能出现在闭包
    参数列表里）。

    这里原先写反了方向：从 brace 往回扫，把遇到的**第一个** `;` 当作
    「已离开表达式」的信号。但 `let f = || { ... };` 里的 `;` 在闭包
    **之后**，扫到的是上一条语句的分号，于是闭包体永远被判成普通块
    ——实测 `|| { unsafe { set_var(..) } }` 的三种闭包形态全部漏报。
    正确做法是**向前**找收尾竖线，`;` 只能作为「配对竖线之前不得
    出现」的禁字符，不能当作终止条件。

    普通表达式里的竖线（如 `a | b`）不会误判：其后没有 `{`，
    不构成 brace。
    """
    i = brace - 1
    while i >= 0 and src[i] in " \t\r\n":
        i -= 1
    if i < 0 or src[i] != "|":
        return None
    # 第二道：从收尾竖线往前找配对的起始竖线，其间不得出现
    # `;`/`{`/`}`（这些字符不可能出现在闭包参数列表里）。
    #
    # 这道检查在**合法 Rust 上几乎不可达**——实测去掉它，自检的
    # 15 条用例与全仓扫描结果**完全不变**。原因：凡 `|` 紧邻 `{`
    # 的合法语法只有闭包；match 的 or-pattern 是 `A | B => {`，
    # 紧邻 `{` 的是 `>`，第一道检查就拦下了。
    #
    # 保留它是为了防御**非 Rust 文件**与未来的语法扩展（`let`-链、
    # 生成代码等），而非覆盖已知输入。留此记录是为了避免下次变异
    # 验证时把它当成「漏网的失效」反复排查——它确实是不可达的。
    j = i - 1  # 从收尾竖线往前找配对的起始竖线
    while j >= 0:
        c = src[j]
        if c in ";{}":
            return None
        if c == "|":
            return CLOSURE_LABEL
        j -= 1
    return None


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
        # 本次 set_var 所在的最内层块（而非函数体）：`unsafe { … }`
        # 自带一层花括号，用函数体当基准会把紧邻语句之间的每一次
        # `unsafe` 包装都当成不同块，误判为「不同作用域」。
        _chain = innermost_block_chain(src, m.start())
        scope_start = _chain[-1] if _chain else body_start
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
            if _in_same_block(src, scope_start, r.start())
        ]
        in_closure = _pos_in_closure(src, m.start())
        # 任一 remove 同时满足三个条件才算数：
        #   (a) 针对**同一个变量名**（var_name 已在上文取出）
        #   (b) 出现在本次 set_var **之后**
        #   (c) 出现在 barrier（assert/panic/unwrap）之前
        #
        # 这里原先只判了 (c)，漏了 (a)(b) 两件，两处都造成真实漏报：
        #
        # 1. **没比对变量名**。于是
        #      set_var("K", …); remove_var("OTHER"); assert!(…);
        #    被当成「K 已清理」而豁免。实测探针报出此漏报。
        #    讽刺的是判据 (a) 在守卫那侧**早就实现了**（GUARD_CONS
        #    TRUCT_RE 按变量名匹配），remove 侧却一直只看文本——同一个
        #    概念存在两套标准。守卫侧已经为这件事写过教训（「守卫只覆盖
        #    它自己那个名字」），remove 侧重蹈。教训要落在**每一处**
        #    同类判定上，不能只落在一处。
        #
        # 2. **没要求 remove 在本次 set_var 之后**。于是
        #      remove_var("K"); set_var("K", …); assert!(…);
        #    被豁免——清理早已跑完，这次设值没人管。
        #
        # 另外原实现用 r_body.find(r.group(0)) 做文本定位，而 group(0)
        # 只是 "env::remove_var("，对**任意变量名**都能匹配；同函数有两处
        # 相同文本时第二次还会命中第一次的位置。改用 match 自带偏移。
        #
        # 变量名读不出来（动态拼接等）时不豁免：宁可多报。
        protected = False
        for r in removes:
            if env_var_name(src, r.end()) != var_name:
                continue  # 清理的是另一个变量名，与本次设值无关
            if r.start() <= m.start():
                continue  # 在本次 set_var 之前，抵消不了它
            # 闭包边界：两者必须**同在闭包内或同在闭包外**。
            #
            # 实测这两条曾被豁免，而它们都是真实泄漏：
            #   set_var 在闭包内、remove_var 在外：闭包体在 f 被调用时
            #     才跑，外层那句在定义处就跑完了，时序无法保证；
            #   set_var 在外、remove_var 在闭包内：清理同样被推迟。
            # 两者都不同时执行，不能互相抵消。
            #
            # 注意这与 _in_same_block 无关：跳过 unsafe 包装后，两者的
            # 最内层块**都是闭包体**，块判定通过，必须单独比闭包归属。
            if _pos_in_closure(src, r.start()) != in_closure:
                continue
            if not BARRIER_RE.search(src[scope_start:r.start()]):
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
    """判据自检：用合成用例验证豁免逻辑既有判别力又不误伤。

    这个门禁的判据改过三次，每次都漏掉了真实缺陷：
      1. 「函数内有守卫即豁免」 → 漏掉不同变量名，以及守卫块已结束的情形
      2. 「文件里有 impl Drop 即整文件免检」→ 让 http_client.rs 的
         9 处真实泄漏、env_guard.rs 自身测试区的裸调用全部免检
      3. 扫描目录硬编码成 ("crates", "webui", "evals") → ide/ 与
         tools/ 里的泄漏完全免检

    三次都没有症状：脚本照常运行、输出「✅ 未发现问题」。只有拿已知
    该报的输入去问「你报不报」，才能发现。所以固化成自检。

    返回未通过的用例描述；空列表表示全部符合预期。
    """
    # 下面的端到端用例要临时换掉 ROOT；不使用 globals()["ROOT"] 是因为
    # 那写法曾让 ROOT 被 Python 当作隐式局部变量，赋值即报
    # UnboundLocalError——一次「看起来在跑」的假失败。
    global ROOT
    cases: list[tuple[str, str, bool]] = [
        # ── protected 判定：(a) 变量名 (b) 顺序 (c) barrier ──
        # 这三条曾全部被豁免。原实现只看「barrier 之前有没有
        # remove_var」，既不比变量名、也不管顺序。
        #
        # 用例里的 True/False 是**语义判定**，不是「旧实现当前返回什么」。
        # 这个区分本轮吃过亏：我先照着旧实现的行为写期望值，结果 15 条
        # 里 6 条其实把漏报固化成了「正确」。期望值必须来自「这段代码在
        # Rust 里的真实语义」——清理有没有配对、清理是否在设值之后、
        # 闭包体和外层是否同时执行——而不是「判据现在怎么说」。
        #
        # 那 6 条的实际分布（已实测，不是推算）：旧实现 15 条错 6 条，
        # 全是漏报，收敛到 4 处根因——
        #   变量名不比对 / 顺序不要求 / 位置靠 find() 文本定位而丢失 /
        #   闭包无边界。同一条判据的多个漏报面会让「6 条失效」与
        #   「4 处修复」两个数字都对，故引用时要说清是哪个口径。
        ("清理早于 set_var", '''
fn t() {
    unsafe { env::remove_var("K") };
    unsafe { env::set_var("K", "x") };
    assert!(true);
}''', True),
        ("remove_var 变量名不同", '''
fn t() {
    unsafe { env::set_var("K", "x") };
    unsafe { env::remove_var("OTHER") };
    assert!(true);
}''', True),
        ("两次 set_var 仅一次清理", '''
fn t() {
    unsafe { env::set_var("K", "x") };
    unsafe { env::remove_var("K") };
    unsafe { env::set_var("K", "y") };
    assert!(true);
}''', True),
        ("清理在 barrier 之后", '''
fn t() {
    unsafe { env::set_var("K", "x") };
    assert!(true);
    unsafe { env::remove_var("K") };
}''', True),
        ("set_var 后立即清理（合法）", '''
fn t() {
    unsafe { env::set_var("K", "x") };
    unsafe { env::remove_var("K") };
    assert!(true);
}''', False),
        ("两次 set_var 两次清理（合法）", '''
fn t() {
    unsafe { env::set_var("K", "a") };
    unsafe { env::remove_var("K") };
    unsafe { env::set_var("K", "b") };
    unsafe { env::remove_var("K") };
    assert!(true);
}''', False),
        ("动态变量名无法判定", '''
fn t() {
    let k = "K";
    unsafe { env::set_var(k, "x") };
    assert!(true);
}''', True),

        # ── 闭包边界 ──
        # 这几条曾全部被豁免。根因：enclosing_fn 只认 `fn`，闭包体
        # 与外层函数体同属一个函数；而两者的执行时机完全不同。
        ("闭包内 set_var、外层清理", '''
fn t() {
    let f = || { unsafe { env::set_var("K", "x") }; };
    unsafe { env::remove_var("K") };
    assert!(true);
}''', True),
        ("闭包内 set_var 无清理", '''
fn t() {
    let f = || { unsafe { env::set_var("K", "x") }; };
    assert!(true);
}''', True),
        ("闭包内成对（合法）", '''
fn t() {
    let f = || {
        unsafe { env::set_var("K", "x") };
        unsafe { env::remove_var("K") };
    };
    assert!(true);
}''', False),
        ("move 闭包 |x| 内 set_var、外层清理", '''
fn t() {
    let f = |x: i32| { unsafe { env::set_var("K", "x") }; };
    unsafe { env::remove_var("K") };
    assert!(true);
}''', True),
        ("外层 set_var、闭包内清理", '''
fn t() {
    unsafe { env::set_var("K", "x") };
    let f = || { unsafe { env::remove_var("K") }; };
    assert!(true);
}''', True),

        # ── 守卫作用域 ──
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
        ("守卫管的是别的变量名", '''
fn t() {
    let _g = EnvVarGuard::set("OTHER", "y");
    unsafe { env::set_var("K", "x") };
    assert!(true);
}''', True),
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
        # 目录名刻意取一个**不在**任何硬编码清单里的名字（如实
        # 曾经用 "crates"）：否则把 SCAN_DIRS 写死回旧值时这条照样
        # 全绿，无法暴露「扫描范围被写死」这个缺陷本身。
        crate = root / "selfcheck_probe_dir" / "nested"
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
            ROOT = root
            # 不改 SCAN_DIRS——正是这里要验证自动发现能捡起新目录。
            probe_findings = scan()
        finally:
            ROOT = real_root
        if not probe_findings:
            # 这条用例同时覆盖两种失效，必须把两者的症状都写出来：
            # 变异「退回硬编码目录清单」与「加回整文件跳过」都会走到这里，
            # 只写一种会让人照着错误方向查。
            failures.append(
                "临时目录里的探针文件未被报出（它既含 impl Drop 又含裸 "
                "set_var，且放在一个不在任何硬编码清单里的目录下）。两种 "
                "可能：(a) scan() 仍有整文件跳过逻辑——impl Drop 只对它"
                "自己那枚守卫变量生效，同文件的裸 set_var 与它无关；"
                "(b) 扫描目录被写死成固定清单，新目录被静默漏扫。"
                "先用 print(_scan_dirs()) 确认根因再改"
            )

    # 豁免边界必须是**真实**的 `#[cfg(test)]` 行，不能是随手写的数字。
    # 只能偏小（滞后），不能偏大（超前）：
    #   超前 → 豁免区伸进测试区，本该报出的泄漏被盖掉（漏报）；
    #   滞后 → 额外检查了几行实现代码，最多报噪音（不漏报）。
    # 即「宁可吵不可静默」。故只判 `actual < limit`。
    #
    # 注：滞后在本文件上恰好无害——实测实现区的末一处裸调用在第 150 行，
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

    # 本文件头部的「刻意不判定」清单曾两次与实现脱节，且两次都是
    # **文档承诺了、实现里根本没有**：
    #   1. 「刻意不判定 set_var 出现在生产代码里」——实现对生产代码
    #      同样判定（探针：无清理 → 报出 1 条）。
    #   2. 「纯串行测试二进制（tests/ 下每个文件独立进程）里的 set_var」
    #      —— 实现里没有任何按路径的豁免。
    #
    # 文档写着「刻意不判定」，读代码却找不到对应逻辑，是最坏的一种
    # 状态：下一个人要么相信文档、以为 tests/ 下永远不会被报（于是
    # 在那里放心写裸 set_var），要么不信、去找那条不存在的豁免。
    #
    # 把真实行为钉在这里。判据语义若有意改动，这里必须同步改。
    # 用 pub fn 而非 fn：生产代码的函数通常是 pub，用测试里的 fn 形态
    # 会让「这是生产代码」的意图丢掉。
    for name, src, want_reported in (
        ("生产代码里的裸 set_var（应报出：实现并不豁免生产代码）", '''
pub fn setup() {
    unsafe { env::set_var("K", "x") };
    do_work();
}''', True),
        ("生产代码里配对清理的 set_var（合法）", '''
pub fn setup() {
    unsafe { env::set_var("K", "x") };
    unsafe { env::remove_var("K") };
    do_work();
}''', False),
    ):
        got = bool(check_panic_safe_cleanup(
            blank_noncode(src, keep_strings=True), "<self-check>"))
        if got != want_reported:
            failures.append(
                f"{name}：期望{'报出' if want_reported else '豁免'}，"
                f"实际{'报出' if got else '豁免'}——文件头部的"
                "「刻意不判定」清单已与实现脱节，请同步更正"
            )

    return failures


def scan() -> list[dict]:
    findings: list[dict] = []
    # 每次调用都重新发现，而不是读模块级的 SCAN_DIRS：自检会临时替换
    # ROOT，用缓存值会扫到真实仓库去，既测不到东西又可能污染结果。
    for base in _scan_dirs():
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
            # 因一个无关的 impl Drop 而让 9 处 set_var 全部免检（修复前
            # 的状态，现已全部改用 EnvVarGuard），crates/sisyphus-types/
            # src/env_guard.rs 同理。
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
