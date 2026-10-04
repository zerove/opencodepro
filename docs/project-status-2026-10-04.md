# 项目状态报告 2026-10-04

> 范围：`sisyphus-tools-build` 的多语言 `LanguageBackend::test()` 链路。
> 起点是上一轮遗留的 vitest 解析器重写，过程中用真实工具链又挖出四个更严重的问题。
> 结论：**六个缺陷全部修复并有端到端验证**，根因判定式本身也已修复。

## 0. 一句话结论

`sisyphus-cli` 用 `if r.failed == 0 { "ok" }` 判断测试成功，
所以**任何解析器只要报 0 失败，构建期失败就会被显示成「测试全部通过」**。
这不是某个解析器的 bug，而是判定式本身缺一个「测试是否真的跑过」的维度。

本轮已把判定式本身修掉：拆出 `test_status_label()`，区分
`ok` / `FAILED` / `no tests ran` 三种状态，并配 6 条单元测试。

## 1. 缺陷清单（按严重度）

| # | 语言 | 现象 | 根因 |
|---|------|------|------|
| 1 | Node | `Tests 1 failed \| 1 passed (2)` 被报成 `passed=2 failed=0` | 旧解析器先匹配 `passed`，整行数字被当成 passed 的值 |
| 2 | Python | **全部通过**的项目报 `passed=0` | `strip_suffix(" passed")` 对 `1 passed in 0.36s` 永不命中 |
| 3 | Node | 套件加载失败（`Failed Suites`）守卫永不触发 | 该标记在 **stderr** 且带 ANSI，原守卫只看 stdout |
| 4 | Go | 多包工程里单个包构建失败被漏判 | 判据是「没有 `--- PASS` 行」，但其余包照常有 PASS |
| 5 | Node | 失败用例名带上 ANSI 转义码 | 解析前没去色 |
| 6 | Java | **全部通过**的 Maven 工程报 `passed=0`；编译失败显示为「0 failed 全部通过」 | ① 汇总行带 `[INFO]` 前缀而解析用 `starts_with`；② 数字格式是 `1,` 带千分位逗号；③ `test()` 直接丢弃 `_success`，无编译失败守卫 |

其中 #2 最反直觉：**测试真的全过了，却报 0 个测试**，
再经 `failed == 0` 判定，显示成「0 个测试全部通过」。

#6 是收尾时用真实 `mvn` 跑出来的，**与 #2 同样的表象（全部通过报 0），
但此前完全没被注意到**——因为它既不在首轮修复清单里，也没有任何守卫覆盖它。
教训：5 个语言后端里，只有 3 个有 e2e 守卫；`java` 和 `frontend` 是裸的。

## 2. 每条结论的实测依据

### 2.1 pytest 单项全过（缺陷 #2）

```
$ python3 -m pytest                      # 单个通过的用例
====== 1 passed in 0.36s ======
```

整行以 ` in 0.36s` 结尾，`strip_suffix(" passed")` 不命中 → `passed` 保持 0。

修复后改为：按逗号切分 → 取首段数字 → 剩余部分作为标签匹配，
并要求行尾是 pytest 的耗时尾巴（` in 0.36s`）以排除无关行。
`error` 归入 failed（fixture 报错场景），`skipped` 归入 ignored。

### 2.2 vitest 混合结果（缺陷 #1、#5）

```
$ npx vitest run
⎯⎯⎯⎯⎯ Failed Tests 1 ⎯⎯⎯⎯⎯
 FAIL  fail.test.js > fails on purpose
 Test Files  1 failed | 1 passed (2)
      Tests  1 failed | 1 passed (2)
```

修复前：`passed=2 failed=0`。修复后：`passed=1 failed=1`，失败名 `fails on purpose`（无 ANSI）。

注意 `Test Files` 那行也是 `1 failed | 1 passed (2)`，
所以解析必须**只认 `Tests` 行**，否则文件级计数会污染用例级计数。

### 2.3 vitest 套件加载失败（缺陷 #3）

```
⎯⎯⎯⎯ Failed Suites 1 ⎯⎯⎯
 FAIL  bad.test.js [ bad.test.js ]
 Test Files  1 failed (1)
      Tests  no tests
```

套件级 `FAIL` 行**没有 `>`**（`[ ... ]` 里是套件名不是用例名），
而用例级是 `FAIL <file> > <case>`。
因此失败名提取用「`FAIL ` 前缀 + 行内含 `>`」作判据，
天然排除套件失败，不会把它混进用例失败列表。

### 2.4 go 多包部分失败（缺陷 #4）

```
$ go test ./...
# probe/badpkg
badpkg/badpkg.go:3:17: undefined: undefinedFunc
FAIL	probe/badpkg [build failed]
ok  	probe/goodpkg	0.003s
FAIL
```

stdout 里既有 `--- PASS` 也有 `[build failed]`。
「没有 PASS 行」判据在这里必然漏判，改用明确的 `[build failed]` 标记。

### 2.5 Maven 全部通过（缺陷 #6）

```
$ mvn -q test
[INFO] Tests run: 1, Failures: 0, Errors: 0, Skipped: 0
```

旧解析器用 `line.starts_with("Tests run:")`，而真实行以 `[INFO] ` 开头，
所以**永远匹配不到**，`passed` 保持 0。
另外数字写作 `1,` 而非 `1`，直接 `parse::<usize>()` 也会失败。

修复：先剥掉 `[INFO]` / `[ERROR]` 日志前缀，再按 `,` 切分取各项数值。
`Errors` 计入 `failed`（JVM 级错误同样是没跑成功），
并对整个输出取汇总**最大值**，避免类级统计与最终汇总重复累加。

### 2.6 Maven 编译失败（缺陷 #6）

```
[ERROR] COMPILATION ERROR :
[INFO] BUILD FAILURE
```

编译失败时**没有任何 `Tests run` 行**，而 `test()` 原本直接丢弃 `_success`，
于是「编译失败」与「零用例」一起退化成 `passed=0 failed=0`。
修复：`test()` 保留 `success` 并识别 `COMPILATION ERROR`，返回 `Err`。

## 3. 测试过程中发现并修掉的自身错误

写 e2e 用例时我自己也犯了两个错，值得记下来：

1. **`go_package` 写出的文件没有 `.go` 后缀** → `go test ./...` 直接跳过该目录，
   探针静默地什么都没测。修法：文件名补后缀，`_test.go` 才真正执行用例。
2. **vitest 解析器把 ANSI 留在失败名里** → 我最初以为守卫没生效，
   实际是判据被转义码拆开（`Tests` 变成 `\e[2m Tests \e[22m`）。
   修法：**在解析器内部去色**，而不是依赖调用方。

这两条都属于「测试看起来在跑，实际什么都没验证」，
和本次修的生产缺陷是同一类问题。

## 4. 验证结果

| 层级 | 命令 | 结果 |
|------|------|------|
| crate 单测 | `cargo test -p sisyphus-tools-build` | 110 passed / 0 failed |
| Java 单测 | `cargo test -p sisyphus-tools-build --lib java` | 5 passed / 0 failed |
| CLI 判定单测 | `cargo test -p sisyphus-cli --lib build::` | **9 passed / 0 failed**（6 条判定 + 3 条提示语） |
| 真实工具链 e2e | `... --test language_backend_guards -- --ignored` | **13 passed / 0 failed**（原 11 + 新增 2 条 Maven） |
| 严格 clippy | `cargo clippy -p sisyphus-tools-build -p sisyphus-cli --all-targets -- -D warnings` | 无告警 |
| 格式 | `cargo fmt --all --check` | 通过 |
| 全工作区 | `make test-all` | **4762 passed / 0 failed / 25 ignored** |
| CI job 确切命令 | `cargo test -p sisyphus-tools-build --test language_backend_guards -- --ignored --test-threads=1` | 13 passed / 0 failed |
| `audit_ignored.py` | `python3 scripts/audit_ignored.py` | **exit 0**（修复前 exit 1，`audit-sources` 长期红） |

全工作区计数可完整对账：相对上轮基线 4750/23，
本轮新增 12 条通过（Java 3 + CLI 判定 6 + 提示语 3）、2 条 ignored（两条新 Maven e2e），
**没有其他变动**。

e2e 覆盖的场景（均真实拉起 `go` / `python3 -m pytest` / `mvn` 子进程）：

- Go：单包编译失败、多包部分失败、全通过、真实用例失败
- Python：单项全过、混合结果、跳过、收集错误（普通）、收集错误（路径含 `passed`）、fixture 报错、真实用例失败
- Java/Maven：编译失败返回 `Err`、全通过得到 `passed=1`

### 4.1 CLI 判定的真实 CLI 验证

```
$ sisyphus build test --lang python -C <空目录>
Test result: no tests ran. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
提示：零用例。可能是空工程、没有测试文件，或过滤器（如 -k/-Dtest）没命中任何用例。

$ sisyphus build test --lang python -C <2 个通过>
Test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ sisyphus build test --lang python -C <1 通过 1 失败>
Test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
Failures:
    test_bad.py::test_b - AssertionError: boom
Error: 1 test(s) failed          # 退出码 1
```

其中「全部 skip」仍报 `ok`（与 cargo / pytest 一致）——跳过是测试作者的显式意图，
降级成 `no tests ran` 反而会在 CI 上制造噪声信号。

### 4.2 改提示语时自己引入的回归（已修）

第一版提示语是**统一**的「可能是空工程、没有测试文件，或过滤器没命中任何用例」。
实测 `--lang fe` 后发现这是**错的**：

```
$ sisyphus build test --lang fe -C <有 index.html 的目录>
Test result: no tests ran. 0 passed; 0 failed; ...
提示：零用例。可能是空工程、没有测试文件，或过滤器（如 -k/-Dtest）没命中任何用例。
```

`frontend` 后端的 `test()` 恒返回全 0（HTML/CSS 本就无测试步骤），
对它说「过滤器没命中」是凭空给建议。修法：提示语按 `backend.name()` 分流。

```
$ sisyphus build test --lang fe -C <同上>
Test result: no tests ran. 0 passed; 0 failed; ...
提示：`frontend` 后端没有测试步骤，HTML/CSS 不做单元测试。

$ sisyphus build test --lang python -C <空目录>
Test result: no tests ran. 0 passed; 0 failed; ...
提示：零用例（后端 `python`）。可能是空工程、没有测试文件，或过滤器没命中任何用例。
```

注意这里与本轮修的解析器 bug 同源：**不能只靠计数反推语义**。
零计数的成因不同（没测试步骤 vs 过滤器滤光），
所以改成让后端自报身份，而不是让 CLI 猜。

### 4.3 `audit-sources` CI job 长期失败的根因（顺带修掉）

加 CI job 时发现 `scripts/audit_ignored.py` **退出码是 1**，即 `audit-sources` job
**一直红着**（已用 `git stash` 确认早于本轮改动，非本次引入）。
两处独立缺陷，都属「判据写错导致假阳性」：

1. **`blank_comments()` 不认 Rust raw string**。它只跳普通 `"` 字符串，
   于是 `rust_backend_test_e2e.rs` 里 `r#"..."#` 包住的**探针工程源码**
   （内含模拟的 `#[ignore = "演示 ignored"]`）被当成该文件真实的忽略点。
2. **`ACCEPTED` 白名单词太窄**。只认 `toolchain`/`installed`，
   于是 `requires real python3 + pytest`、`requires real mvn + jdk`
   这类**更具体、更诚实**的理由反而被判非法。

修复：`blank_comments()` 增加 raw string 支持（`r"..."` / `r#"..."#` / `r##"..."##`），
`ACCEPTED` 增加显式外部可执行文件名与测量型用例的词。
结果：`rejected 12 → 0`，**exit 1 → 0**。

双向实测确认没把门放松：
- 注入 `#[ignore = "测试当前失败，暂时跳过"]` → 仍被判非法、exit 1
- 真实存在的 2 处探针源码 `#[ignore]` → 不再误报

**这与本轮修的守卫缺陷是同一个思维错误**：用错误的判据去推断意图，
结果既漏报（Java 编译失败被当成全通过）又误报（合法的 pytest ignore 被当非法）。
判据必须锚在**意图**上，而不是字符串表面。

## 5. 环境噪音（非代码缺陷）

- **rustc 会在编译 `sisyphus-core` 时随机 SIGABRT**，成因**至今未定**。
  表现：`process didn't exit successfully: ...rustc --crate-name sisyphus_core ...
  (signal: 6, SIGABRT)`，退出码 101。

  **实测事实**（可复现的部分）：第一次 `make test-all`（Makefile 里已带
  `CARGO_BUILD_JOBS=1`）SIGABRT，第二次跑完全相同的命令一次通过
  （4759 passed / 0 failed / exit 0）。

  **不成立的推论**（本轮已纠正）：旧记录写「故这不是并发 OOM，
  而是概率性崩溃，与 `-j 1`/`-j 2` 都对不上」。这**超出了证据**——
  崩溃发生在**rustc 编译期**（不是测试运行期），而此时
  `--test-threads` 根本不参与；因此`-j 1` 确实**未覆盖到**崩溃场景，
  但“与并发无关”仍未得到证明——**编译期也存在其他并发**
  （多任务争用内存带宽/内存分配器内部并行），本轮**未做二分取证**，
  且原始日志已被后续运行覆盖。

  已排除 / 已核查：
  - **不是内核 OOM killer**：崩溃时 `available` 约 13 GiB（充足），
    且 `dmesg` 中无任何 `oom-kill` / `out of memory` 记录
    （注：`dmesg` 缓冲区已轮转，故这是**弱证据**，非决定性排除）
  - `RUSTC_WRAPPER` / `RUSTFLAGS` / `sccache` / `CARGO_BUILD_JOBS` 均无外部干扰设置
  - 机器 19 GiB 内存 + 12 核；存在 5 GiB swap（已用 61 MiB）

  当前处置：`Makefile` 的 `CARGO_BUILD_JOBS=1` **保留**（它防的是
  另一个已实测存在的场景：多二进制并行链接 OOM，那是**真**发生的）。
  遇到 rustc SIGABRT 时：**直接重跑**。

  **下次复现时应立即取证**（本轮的教训——现场一丢就再也查不出）：

  | 步骤 | 命令 | 作用 |
  |------|------|------|
  | 1 | `make rustc-abort-forensics` | 自动循环重编 + 保留 core（**推荐**） |
  | 2 | `coredumpctl list` | 事后取现场（`core_pattern` 已配 systemd-coredump） |
  | 3 | `coredumpctl info <PID>` | 看崩溃线程栈，直接判定 rustc 内部哪个阶段 |

  根因已查清：`ulimit -c` 当前为 **0**，即即便崩溃也**不留任何现场**——
  这正是上一轮“日志已被覆盖、无法再取证”的直接原因。

  `make rustc-abort-forensics` 已实跑验证（`ROUNDS=1`，4m24s）：

  ```
  ⚠  ulimit -c = 0；已在本目标子 shell 内覆盖为 unlimited
     core_pattern: |/usr/lib/systemd/systemd-coredump %P %u %g %s %t %c %h %d %F
  ✓ coredumpctl 可用（复现后用 coredumpctl info <PID> 看栈）
  === 循环重编 1 轮（JOBS=1，每轮强制 touch 重编）===
    [ 1/1] ok
  === 1 轮均未复现 —— 这是**阴性结果**，不证明成因 ===
  ```

  两个设计要点（均由实跑暴露后修正）：
  - 每轮 `touch crates/sisyphus-core/src/lib.rs` 强制**真重编**，
    否则 cargo 命中缓存直接返回，循环毫无意义
  - 输出重定向到 `$JCODE_SCRATCH_DIR/forensics/round-N.log`，
    保留完整崩溃输出而不淹没终端

  `JOBS=N` 可调构建并发做二分：`JOBS=1` 复刻 `make test-all` 场景，
  `JOBS=12` 对照全并行。
- `sisyphus-core/tests/executor_failure_semantics.rs:48` 有 1 个
  `fetch_update` 弃用告警（rustc 1.99）。不影响测试，已在同批清理为 `try_update`。

## 6. 提交

`6c3a52e fix(build): 修正 Go/Python/Node 测试守卫漏报与计数错误`
（+936 / −66，7 文件；pre-commit 检查通过）

## 7. 建议的后续动作

### 7.1 修判定式本身（已完成，见第 4.1 节）

`if r.failed == 0 { "ok" }` 是这次所有缺陷的放大器。
已拆出 `test_status_label()` 并区分三种状态；后续可考虑进一步给
`TestResults` 增加一个**执行状态**维度，例如：

```rust
pub enum TestOutcome {
    Ran { passed: usize, failed: usize },  // 确实跑了用例
    NoTests,                                // 跑完了但零用例
    NotExecuted,                            // 编译/收集/加载失败
}
```

让调用方不再需要「靠 failed==0 反推有没有跑过」。

### 7.2 e2e 测试进 CI（已完成）

原先这 13 条全是 `#[ignore]` 且**无任何 CI job 跑它们**——
这正是本轮 6 个缺陷能长期存活的原因（Java 后端干脆一条守卫都没有）。

已新增 `toolchain-e2e` job（`.github/workflows/ci.yml`）：
装 go 1.22 / python 3.11 + pytest / temurin 17 + Maven，
跑 `--ignored --test-threads=1`，并打印工具链版本便于排障。
`timeout-minutes: 20`（Maven 首次要下插件与依赖）。

附带一条守卫：断言 `rust_backend_test_e2e.rs` 的**自身**测试没被加 `#[ignore]`
（它在 `test-light` 里已执行）。判据必须先剥掉 `r#"..."#` 探针源码字符串，
否则会把探针里模拟的 `#[ignore]` 当成本文件的属性而误报——已双向实测。

### 7.3 补齐剩余后端的守卫

本轮教训是**守卫覆盖不全**：在写这份报告时，我曾错误地写下
「`rust` 与 `frontend` 目前仍无真实工具链 e2e」。
**实际上 `rust_backend_test_e2e.rs` 早已存在**，含 7 条**不带 `#[ignore]`** 的
真实 `cargo test` e2e（已实测 7 passed / 0 failed），覆盖文档测试不计入 `passed`、
`--no-fail-fast` 缺省导致后续目标不运行等单测覆盖不到的路径。

因此真实结论是：**6 个后端里 5 个已有真实工具链 e2e**（rust / go / python / java / node），
只剩 `frontend` 没有——而它本就没有测试步骤（`test()` 直接返回全 0）。
4.2 节已实测：它不受新提示语影响（提示语按 `backend.name()` 分流，
会明确说「没有测试步骤」），故**无需额外豁免路径**，这一点已闭环。

> 记录这个错误是因为它与本轮修的 bug 同类：
> **用「我记得」代替「我查过」**。当时只 grep 了 `language_backend_guards.rs`，
> 没有列出 `tests/` 目录全貌就下了结论。

### 7.4 ~~`.cargo/config.toml` 补并发限制~~（已作废，勿执行）

旧建议是“给 `.cargo/config.toml` 的 `test-all` 补上 `CARGO_BUILD_JOBS=1`”。

**作废理由**：`CARGO_BUILD_JOBS=1` 已存在于 `Makefile` 的全部重目标上
（`git show 6c3a52e:Makefile` 确认早于本轮改动）。往 `.cargo/config.toml`
再加一份只是**重复已有设置**，没有任何新约束。

同时确认 `.cargo/config.toml` **无 `[env]` 段**，本轮未做任何改动。

> **归因未查明，不得当作已解决**：本轮 `make test-all`（已带 `-j 1`）
> 出现过 SIGABRT，同一条命令重跑即通过。但这**不足以**断定
> 「`-j 1` 不能根除它」——构建级并发（`CARGO_BUILD_JOBS`）已被限制，
> **测试线程级并发（`--test-threads`，默认 = CPU 核数）并未被限制**，
> 而后者在 `make test-all` 路径上同样生效。二者在观测上等价，
> 且 SIGABRT 原始日志已被后续运行覆盖、无法再取证。
>
> 因此正确说法是：**SIGABRT 的成因至今未定**，
> 既不能称它为并发问题，也不能称它与并发无关。
> 遇到 SIGABRT 直接重跑；真要根治，应先取证再动手——
> 保留失败现场（`make scan-flaky` 或 `RUST_BACKTRACE=1` 重跑），
> 再分别试 `--test-threads=1` 与 `-j 1` 做二分。
>
> 本轮已顺手补上唯一真正缺失的一处：`Makefile` 的 `scan-flaky` 目标及其伴随的
> `cargo test --workspace --lib --no-run` **原先未带** `CARGO_BUILD_JOBS=1`，
> 而它是唯一会同时产出多个重型测试二进制的目标，已加上。
>
> 其余未受限的调用经核查**属有意为之**：`test-ci` 刻意镜像 CI
> （GitHub runner 16 GiB，本就无此限制）；`test-core-quick` / `test-core-min`
> 是单包 lib 测试，只产出一个二进制，不触发并行链接。

## 8. 明确不在本次范围

- bridge / sandbox / sandboxd / tests crate（工作区排除，见 `.cargo/config.toml`）
- 其他语言后端之外的生产代码路径
- `TestOutcome` 枚举重构（7.1 的下一阶段）——本次只修展示判定，
  改成 `TestResults` 的新维度需审计全部 6 个构造点与消费方
