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
| CLI 判定单测 | `cargo test -p sisyphus-cli --lib build::` | 6 passed / 0 failed |
| 真实工具链 e2e | `... --test language_backend_guards -- --ignored` | **13 passed / 0 failed**（原 11 + 新增 2 条 Maven） |
| 严格 clippy | `cargo clippy -p sisyphus-tools-build -p sisyphus-cli --all-targets -- -D warnings` | 无告警 |
| 格式 | `cargo fmt --all --check` | 通过 |
| 全工作区 | `make test-all` | **4759 passed / 0 failed / 25 ignored** |

全工作区计数可完整对账：相对上轮基线 4750/23，
本轮新增 9 条通过（Java 3 条 + CLI 判定 6 条）、2 条 ignored（两条新 Maven e2e），
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

## 5. 环境噪音（非代码缺陷）

- **rustc 会在编译 `sisyphus-core` 时随机 SIGABRT**，与并行度无关。
  表现：`process didn't exit successfully: ...rustc --crate-name sisyphus_core ...
  (signal: 6, SIGABRT)`，退出码 101。

  本次实测**直接推翻了旧记录里“只有 `-j 1` 稳定”的结论**：
  第一次 `make test-all`（Makefile 里已带 `CARGO_BUILD_JOBS=1`）**仍然 SIGABRT**，
  第二次重跑完全相同的命令则一次通过（4759 passed / 0 failed / exit 0）。
  也就是说这不是并发 OOM，而是**概率性崩溃**，与 `-j 1` / `-j 2` 都对不上。

  当前状态：机器 19 GiB 内存（free 显示 13 GiB available）+ 12 核，
  已排除 OOM（available 充足时崩溃）；`RUSTC_WRAPPER` / `RUSTFLAGS` /
  `sccache` / `CARGO_BUILD_JOBS` 均无外部干扰设置。
  故 `Makefile` 的 `CARGO_BUILD_JOBS=1` **保留**（它至少避免了已知的最坏情形），
  但**不再声称它能根除此崩溃**。

  遇到时的处置：**直接重跑**，不要试图调并发参数。
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

### 7.2 e2e 测试进 CI

`language_backend_guards.rs` 13 条目前全是 `#[ignore]`。
它们依赖 `go` / `python3 + pytest` / `mvn`，价值很高
（本次两个自身 bug 和 Java 缺陷都是它们抓出来的）。
建议在 CI 加一个带工具链的 job 跑 `--ignored`，而不是长期挂着。

### 7.3 补齐剩余后端的守卫

本轮教训是**守卫覆盖不全**：`rust` 与 `frontend` 目前仍无真实工具链 e2e。
`rust` 后端走 `cargo test`，但它自身就在 Rust 仓库里，自举成本高；
`frontend` 没有测试步骤，但它的零用例行为会被新判定式标成 `no tests ran`
（语义上偏负面，建议单独考虑给它一条豁免路径）。

### 7.4 清理环境告警

- `executor_failure_semantics.rs` 的 `fetch_update` → `try_update`（已在同批清理）
- `.cargo/config.toml` 的 `test-all` 补上 `CARGO_BUILD_JOBS=1`
  （TODO 10.3 说 Makefile 已加，但直接敲 `cargo test-all` 仍会撞并行链接）

## 8. 明确不在本次范围

- bridge / sandbox / sandboxd / tests crate（工作区排除，见 `.cargo/config.toml`）
- 其他语言后端之外的生产代码路径
- `TestOutcome` 枚举重构（7.1 的下一阶段）——本次只修展示判定，
  改成 `TestResults` 的新维度需审计全部 6 个构造点与消费方
