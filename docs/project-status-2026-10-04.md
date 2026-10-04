# 项目状态报告 2026-10-04

> 范围：`sisyphus-tools-build` 的多语言 `LanguageBackend::test()` 链路。
> 起点是上一轮遗留的 vitest 解析器重写，过程中用真实工具链又挖出三个更严重的问题。
> 结论：**五个缺陷全部修复并有端到端验证**，全工作区回归 4750 通过 / 0 失败。

## 0. 一句话结论

`sisyphus-cli` 用 `if r.failed == 0 { "ok" }` 判断测试成功，
所以**任何解析器只要报 0 失败，构建期失败就会被显示成「测试全部通过」**。
这不是某个解析器的 bug，而是判定式本身缺一个「测试是否真的跑过」的维度。

## 1. 缺陷清单（按严重度）

| # | 语言 | 现象 | 根因 |
|---|------|------|------|
| 1 | Node | `Tests 1 failed \| 1 passed (2)` 被报成 `passed=2 failed=0` | 旧解析器先匹配 `passed`，整行数字被当成 passed 的值 |
| 2 | Python | **全部通过**的项目报 `passed=0` | `strip_suffix(" passed")` 对 `1 passed in 0.36s` 永不命中 |
| 3 | Node | 套件加载失败（`Failed Suites`）守卫永不触发 | 该标记在 **stderr** 且带 ANSI，原守卫只看 stdout |
| 4 | Go | 多包工程里单个包构建失败被漏判 | 判据是「没有 `--- PASS` 行」，但其余包照常有 PASS |
| 5 | Node | 失败用例名带上 ANSI 转义码 | 解析前没去色 |

其中 #2 最反直觉：**测试真的全过了，却报 0 个测试**，
再经 `failed == 0` 判定，显示成「0 个测试全部通过」。

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
| 真实工具链 e2e | `... --test language_backend_guards -- --ignored` | 11 passed / 0 failed |
| 严格 clippy | `cargo clippy -p sisyphus-tools-build --all-targets -- -D warnings` | 无告警 |
| 格式 | `cargo fmt --all --check` | 通过 |
| 全工作区 | `cargo test-all -j 2` | **4750 passed / 0 failed / 23 ignored** |

e2e 覆盖的场景（均真实拉起 `go` / `python3 -m pytest` 子进程）：

- Go：单包编译失败、多包部分失败、全通过、真实用例失败
- Python：单项全过、混合结果、跳过、收集错误（普通）、收集错误（路径含 `passed`）、fixture 报错、真实用例失败

## 5. 环境噪音（非代码缺陷）

- **`cargo check-all` / `cargo test-all` 并行时会撞 rustc/lld 的 SIGABRT**，
  表现为 `0x0` ICE 或 `linking with cc failed: Resource temporarily unavailable`
  （线程创建 EAGAIN）。19 GiB 内存下 `-j 2` 稳定、`-j 1` 更稳。
  这是环境约束，**不是本次改动引入的**（单 crate 检查与全量测试均通过）。
- `sisyphus-core/tests/executor_failure_semantics.rs:48` 有 1 个
  `fetch_update` 弃用告警（rustc 1.99），不影响测试，未在本次范围内处理。

## 6. 提交

`6c3a52e fix(build): 修正 Go/Python/Node 测试守卫漏报与计数错误`
（+936 / −66，7 文件；pre-commit 检查通过）

## 7. 建议的后续动作

### 7.1 修判定式本身（优先级最高）

`if r.failed == 0 { "ok" }` 是这次所有缺陷的放大器。
建议给 `TestResults` 增加一个**执行状态**维度，例如：

```rust
pub enum TestOutcome {
    Ran { passed: usize, failed: usize },  // 确实跑了用例
    NoTests,                                // 跑完了但零用例
    NotExecuted,                            // 编译/收集/加载失败
}
```

让调用方不再需要「靠 failed==0 反推有没有跑过」。
否则每加一个语言后端，都要重新踩一遍同样的坑。

### 7.2 e2e 测试进 CI

`language_backend_guards.rs` 11 条目前全是 `#[ignore]`。
它们依赖 `go` / `python3 + pytest`，价值很高（本次两个自身 bug 就是它们抓出来的）。
建议在 CI 加一个带工具链的 job 跑 `--ignored`，而不是长期挂着。

### 7.3 清理环境告警

- `executor_failure_semantics.rs` 的 `fetch_update` → `try_update`
- 给 `.cargo/config.toml` 的 `test-all` 补上 `CARGO_BUILD_JOBS=1`
  （TODO 10.3 说 Makefile 已加，但直接敲 `cargo test-all` 仍会撞并行链接）

## 8. 明确不在本次范围

- bridge / sandbox / sandboxd / tests crate（工作区排除，见 `.cargo/config.toml`）
- 其他语言后端之外的生产代码路径
- 判定式重构（7.1）——建议单独立项，本次只做数据正确性修复
