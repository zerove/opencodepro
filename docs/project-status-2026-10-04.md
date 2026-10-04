# 项目状态报告 2026-10-04

> 范围：`sisyphus-tools-build` 的多语言 `LanguageBackend::test()` 链路。
> 起点是上一轮遗留的 vitest 解析器重写，过程中用真实工具链又挖出四个更严重的问题。
> 结论：**六个缺陷全部修复并有端到端验证**，根因判定式本身也已修复。

## 0. 一句话结论

`sisyphus-cli` 用 `if r.failed == 0 { "ok" }` 判断测试成功，
所以**任何解析器只要报 0 失败，构建期失败就会被显示成「测试全部通过」**。
这不是某个解析器的 bug，而是判定式本身缺一个「测试是否真的跑过」的维度。

本轮已把判定式本身修掉：拆出 `test_status_label()`，区分
`ok` / `FAILED` / `no tests ran` 三种状态，并配 **9 条单元测试**
（6 条判定 + 3 条提示语）。此外：

- 6 个后端解析缺陷全部修复，并用**真实工具链**端到端验证（13 条 e2e，
  覆盖 Go / Python / Maven——**不含 Node/vitest**，Node 侧的 3 个缺陷靠
  单元测试锁定解析器，未拉起真实 npm 进程）；
- e2e 已**接入 CI**（新增 `toolchain-e2e` job）——此前它们全是 `#[ignore]`
  且无任何 CI job 执行，这正是缺陷能长期存活的原因；
- 顺带修复长期失败的 `audit-sources` job（exit 1 → 0）。

**本轮另有两项非预期的收获**（详见第 5 节）：
SIGABRT 的成因**已查明**（链接期 rust-lld 撞 `Committed_AS` 上限），
以及一个真实的数据丢失竞态（`ExecutionManager` drop 时可能丢认知追踪）。

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
| 真实工具链 e2e | `... --test language_backend_guards -- --ignored` | **13 passed / 0 failed**（原 11 + 新增 2 条 Maven；**无 Node**，见第 0 节说明） |
| 严格 clippy | `cargo clippy -p sisyphus-tools-build -p sisyphus-cli --all-targets -- -D warnings` | 无告警 |
| 格式 | `cargo fmt --all --check` | 通过 |
| 全工作区 | `make test-all` | **4763 passed / 0 failed / 25 ignored**（MAKE_EXIT=0，无 OOM/SIGABRT） |
| 嵌套 cargo 闸门 | `... --test rust_backend_test_e2e -- --test-threads=8`，**修复前**对照 | 10 轮失败 1（`LLVM ERROR: out of memory`） |
| 嵌套 cargo 闸门 | 同上，**修复后** | 26 轮 **0 失败**（不声明已根治，见 5.4.2） |
| 认知追踪 flush | `cargo test -p sisyphus-core --test cognitive_trace_persist_test` | 4 passed / 0 failed |
| 追踪竞态探针（修复前） | 30 轮不 flush 直接 drop | **5/30 轮丢失** |
| 追踪竞态探针（修复后） | 30 轮 flush 后 drop | **0/30 丢失** |
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

## 5. 环境噪音：rustc SIGABRT —— **成因已查明（2026-10-04 晚补测）**

> ⚠ 本节在初版报告里写过“成因至今未定”。补测后**已定位**，
> 下文保留推理过程以便复核。

### 5.1 症状

```
error: linking with `cc` failed: exit status: 1
  = note: terminate called after throwing an instance of 'std::system_error'
            what():  Resource temporarily unavailable          ← EAGAIN
          PLEASE submit a bug report to .../llvm/llvm-project/issues/...
  Stack dump:
  0.  Program arguments: .../bin/rust-lld -flavor gnu -plugin ...
  ...  rust-lld  ...
  collect2: fatal error: ld terminated with signal 6 [Aborted]
error: could not compile `sisyphus-core` (test "enhanced_dashboard")
```

### 5.2 三个被我写错的地方（初版推论）

| 初版结论 | 实际 |
|----------|------|
| “崩溃发生在 **rustc 编译期**，`-j 1` 已覆盖不到” | 崩在**链接期**（rust-lld），而链接正是 `CARGO_BUILD_JOBS` 管的范围，所以 `-j 1` **确实管到了** |
| “不是并发 OOM，而是概率性崩溃” | **就是提交内存（commit）耗尽**导致的分配失败 |
| “已排除 OOM（available 充足时崩溃）” | **恰恰相反**：撞的是 `Committed_AS` 上限，不是真实 RAM |

第三项是关键教训。两种内存记账完全不同：

```
free 的 available  ≈ 10.4 GiB   ← 真实空闲 RAM（我拿这个当“已排除”的依据）
Committed_AS      ≈ 18.4 GiB / CommitLimit 21.1 GiB (87%)   ← 实际被撞的墙
```

`vm.overcommit_memory = 2`（**严格模式**）+ `overcommit_ratio = 80`，
故 `CommitLimit ≈ RAM × 80%`。当 lld 申请一大块连续虚拟地址空间时，
即使物理内存还有 10 GiB，也会因**提交量超限**直接返回 `EAGAIN`。
这完美解释了“随机”表现：取决于当时 `buff/cache` 与其他进程的提交量。

### 5.3 正确结论与处置

- `CARGO_BUILD_JOBS=1` **不是**无用，它把同时链接的二进制数降到 1，
  确实降低提交峰值（这正是最初加它的原因，结论没变）
- 但它**不保证不撞墙**：单个 lld 自身申请的大块虚拟内存就可能超限
- `make rustc-abort-forensics`（本轮新增，实跑验证可用）现已能保留现场，
  且它**监控的正是 `Committed_AS`**（这是关键，详见 Makefile 注释）

### 5.4 根治手段（**已实测一部分，结论：不能宣布修复**）

本轮真的跑了 A/B 对照，而不是继续推测。`CommitLimit = 21122 MiB`。

#### 第一版实验：有致命对照缺陷，已作废

| 组 | 结果 | 问题 |
|----|------|------|
| dev 默认（`debug=2`） | **EXIT=101 崩溃**，峰值 20261 MiB（95.9%） | 只编了 **8** 个 crate 就撞上 |
| `CARGO_PROFILE_DEV_DEBUG=0` | EXIT=0，峰值 20071 MiB | 编了 **330** 个 crate |

看上去“debug=0 救了我”，但这**不能算数**：两组编译规模差 40 倍。
崩的是 `serde_derive`——一个很小的 proc-macro crate，**不是**大二进制。
拿它和 330 个 crate 的那组比峰值，是在比两件不同的事。
**这版实验作废。**

#### 第二版实验：同条件预热后重测

| 组 | 预热 rc | 测量 rc | 峰值 MiB |
|----|---------|---------|----------|
| dev 默认（`debug=2`） | **101（崩溃）** | 0 | 19803 |
| `CARGO_PROFILE_DEV_DEBUG=0` | 0 | 0 | 18868 |

峰值差 **935 MiB（约 4.7%）**，`debug=0` 确实降低了链接期提交量。

**但这仍证明不了「debug=0 修好了它」**，两个硬伤：

1. **同样配置下结果不一致**：debug=2 这组在预热阶段崩了（崩在
   `futures-macro`），测量阶段却通过了。同一台机器、同一个命令，
   差别只是时间先后——说明这是**概率性、负载相关**故障，
   单次“跑过”不构成证据。
2. **4.7% 不足以跨过那堵墙**。崩溃发生在提交量冲高时，
   降 4.7% 只是把概率往后推，不是消除。

#### 尚未实测（但与已确认成因直接对得上）

| 方案 | 预期 | 状态 |
|------|------|------|
| 切 `mold` | 链接器内存占用大降 | 未实测；`.cargo/config.toml` 已注明需 >16 GB |
| `-fuse-ld=lld` 替代 GNU ld | 同上 | 未实测 |
| 调低 `overcommit_ratio` | 直接抬高 `CommitLimit` | **需 root，改系统全局设置**，项目不应决定 |

前两项都是**链接器选择**，而本轮已确认崩溃正发生在链接期；
第四项本轮已明确排除。**不做系统级修改**。

**如实结论**：SIGABRT 的**成因**已查明（提交内存额度耗尽），
但**根治手段仍未验证**。不要把本节当成“已修复”。

#### 5.4.1 又一处我自己写错的因果

上一轮我在报告里说「`make test-all` 本身已导出
`CARGO_BUILD_JOBS=1`，所以 `cargo_raw` 那处改动对完整 Makefile 路径
可能没有实际作用」。本轮实测**这条判断是错的**：

- 探针确认 `CARGO_BUILD_JOBS=1` **确实**到达测试二进制进程；
- `cargo_raw` 的分支是「调用方已设置则尊重，否则注入」，
  两边都是 `1`，**结果一致，不冲突**；
- 但 `.github/workflows/ci.yml` **根本没设** `CARGO_BUILD_JOBS`，
  所以在 CI 路径下这处守卫**会真正生效**（本地 make 路径下则冗余）。

即：改动在 CI 上有意义，在 `make test-all` 上冗余。我原先把
“冗余”说成“可能没有实际作用”，方向说反了。

### 5.4.2 闸门加错了层级——「已修复」在真实负载下只成立 2/3

上面那句自我纠错写完不到一小时，**最终回归就把自己的修复顶穿了**：
`make test-all` 真实退出码是 **2**，不是 0。失败的是
`rust_backend_test_e2e` 的 2 条用例，错误信息正是本节的根因：

```text
LLVM ERROR: out of memory
collect2: fatal error: ld terminated with signal 6 [Aborted]
```

先要坦白一件事：上一节我说「`cargo_raw` 的改动在 CI 上有意义」，
但**它挡不住这个**。逐层看清后发现**守卫的层级从一开始就错了**：

| 层级 | 控制什么 | 现状 |
|------|---------|------|
| `CARGO_BUILD_JOBS=1`（已注入**单个**嵌套 cargo） | 一个 cargo 内部同时几个 rustc/链接器 | ✅ 管到了 |
| **同时有几个嵌套 cargo 在跑** | 整个测试进程的链接器总数 | ❌ 没人管 |

而 `rust_backend_test_e2e.rs` 的 7 条用例由 libtest **并行**调度，
每条都拉起一整个嵌套 `cargo test`（真跑 rustc + 链接器）。
于是最多有 7 个链接器同时争抢提交内存——`CARGO_BUILD_JOBS=1`
对它们**一个都约束不到**，因为它们分属不同的 cargo 进程。

这也解释了当初那个“正确但被归错因”的观察：
`--test-threads=1` 之所以能救，是因为它限制了 cargo 的**个数**，
而不是因为 `CARGO_BUILD_JOBS` 起了作用。

**修复**：在该测试文件内加一个跨用例的全局 `Mutex` 闸门，
把并发嵌套 cargo 限为 1；并把 `cargo_raw` 与该文件里两处
归因错误的注释一并改正。

对照压测（同一条命令 `--test-threads=8` 刻意制造并行）：

| 版本 | 轮次 | 失败 | 失败时的证据 |
|------|-----:|-----:|-------------|
| 修复前（`git stash` 回退） | 10 | **1** | `LLVM ERROR: out of memory` + `signal 6 [Aborted]` |
| 修复后 | 20 | 0 | — |

对照组失败的**特征与** `make test-all` 下那次**完全一致**，可确认同一根因。

**但不要把「20/20」读成「已根治」**：若真实失败率仍是 10%，
连过 20 次的概率约 **12.2%**；若为 25%，则只有 0.3%。
所以这里只声明「**把并发嵌套 cargo 限为 1 后未再复现**」，
不声明概率已归零。根因仍是全机性的提交内存紧张，
本闸门只消除了**本测试文件内部**的并发放大。


### 5.5 连带发现：认知追踪落盘竞态（**已确定性复现并修复**）

补跑 `make test-all` 时出现（同一轮未再 SIGABRT，编译链接均通过）：

```text
thread 'test_persist_survives_buffer_recreation' panicked at
  crates/sisyphus-core/tests/cognitive_trace_persist_test.rs:194:5:
assertion `left == right` failed
  left: 0
 right: 1
```

上一版报告写“尚未构造出稳定复现”，本轮把它复现出来了——
方法是**去掉测试自带的等待**，直接模拟生产关闭路径：

| 场景 | 结果 |
|------|------|
| 记录后**不 flush 直接 drop**（30 轮） | **5/30 轮丢失** ← 确定性复现 |
| 记录后 `flush_cognitive_traces()` 再 drop（30 轮） | **0/30 丢失** |

**根因**（不是“linker 撞内存”的同源问题）：

1. `record_cognitive_trace` 是**同步函数**，`tx.send()` 后立即返回，
   调用方**没有任何手段 await 到持久化完成**。
2. writer 由 `tokio::spawn` 起的**游离任务**消费；`drop` 关闭 channel
   后它才被唤醒，此时 SQL 写入可能根本没发生。
3. `ExecutionManager` 被 `Arc` 持有，并且**克隆进**
   `SelfProgrammingManager` / `StrategicManager`，
   所以**`Drop` 实现不是可靠刷盘点**——不能靠 drop hook 修。
4. 旧测试用轮询 `wait_history` 等待，且 `let _ = wait_history(..)`
   **丢弃返回值**，轮询超时（1s）也是静默通过。

**修复**：channel 元素从 `(String, entry)` 改为 `TracePersistCmd`：

```text
enum TracePersistCmd {
    Record(Box<(String, CognitiveTraceEntry)>),   // 装箱：见下
    Flush(oneshot::Sender<()>),                  // FIFO 屏障
}
```

`Flush` 指令与 writer 串行处理构成 FIFO 屏障——轮到它时，
前面的 `Record` 必然已落盘；`flush_cognitive_traces()` 等回执，
返回即代表“已持久化”。同时接入 `MySisyphusBridge::shutdown()`，
让 SIGINT/SIGTERM 真实关闭路径也排空队列。

**过程中的两个插曲**：

- clippy 报 `large_enum_variant`（`ExecutionEvent` 比 oneshot sender
  大得多）——**clippy 抓到了我自己的问题**，已装箱。
- 我原先把 `cargo test` 的 `real 0m56s` 当成测试耗时，
  实际 **3 个用例只跑 2.80s**，54s 全是编译。
  **“112s 慢测试”是我自己的误判**，已撤回。

**风险等级**：**生产缺陷**——bridge 关闭时丢跨会话认知追踪历史。
已由 `test_flush_cognitive_traces_survives_recreation`（10 轮）覆盖，
临时探针已删除。

### 5.6 第二次相关失败：嵌套 cargo 撞同一堵墙（**已修复**）

补跑 `make test-all` 时 `rust_backend_test_e2e` 有 3 条失败：

```
LLVM ERROR: out of memory
Allocation failed
collect2: fatal error: ld terminated with signal 6 [Aborted]
```

与 5.1 **同根因**（链接期提交内存耗尽），但触发方式不同：
libtest 默认并行跑测试，每条用例各起一个**完整的嵌套 cargo 构建**，
于是在外层全量并发之上又叠了一层并发。

**已做的改动**：`cargo_raw()` 给子进程注入 `CARGO_BUILD_JOBS=1`
（仅当调用方未设置时），并新增 `run_command_with_env()`。
注入在**子进程**上而非 `std::env::set_var`——后者在多线程测试二进制里不健全。

**修正后的验证**（先回滚又重跑了真实全量负载）：

| 实验 | 结果 |
|------|------|
| 修复后**并行**单跑该 e2e | 7 passed（1.35s） |
| 移除修复后**并行**单跑 | 7 passed ← 单跑无法复现，对照无效 |
| 移除修复 + 人工压测到 commit **99%**（20936/21122 MiB） | 仍 7 passed（压测进程自身抛 `MemoryError`，压力真实） |
| **修复后跑完整 `make test-all`**（真实负载） | **该 3 条全过**，未再出现 `LLVM ERROR` |

结论：前三个实验都**不足以**判定，单跑场景与原失败场景不可比。
真正的判据是最后一行——只有在**外层 4762 用例全量并行**这个
原失败负载下才复现得了，而修复后该负载下通过。
（注意这仍属**单次观察**，不是统计显著的结论；负载相关缺陷本就需要多次复现。）

**另一处自己踩的坑**：本条改动里的 ``` 代码块没标语言，
被 rustdoc 当成 Rust doctest 编译，`LLVM ERROR: out of memory`
不是合法 Rust → `expected one of '!' or '::', found 'ERROR'`。
**我自己引入的失败**，已改成 ```text。教训：文档里的输出样例必须标语言。

真正的处置仍是第 5.4 节：降低链接期提交量（均未实测）。

### 5.7 `toolchain-e2e` job 引入即失败（**已修**）

上一版把 e2e 接进 CI 时写了 `cache: maven`，这个 job **一旦上线必红**：

- `setup-java@v4` 用 `**/pom.xml` 算缓存 key，
  而本仓库**没有签入任何 pom.xml**（Maven 探针是测试里 `TempDir` 现造的）。
- `setup-java/src/cache.ts:96-100` 对“无文件匹配”是 `throw`，**不是** warning：

```text
const fileHash = await glob.hashFiles(pattern.join('\n'));
if (!fileHash) {
  throw new Error(`No file in ${process.cwd()} matched to [${pattern}] ...`);
}
```

已移除该行并在 YAML 里写下原因（避免后人“好心”加回来）。

**Maven 从哪来**：不需要安装步骤。GitHub 的 `ubuntu-latest`
（Ubuntu 24.04 镜像）**预装 Maven 3.9.16**，已核对官方
`actions/runner-images` 的 `Ubuntu2404-Readme.md`。
即之前「Maven 安装来源不明」的疑点，答案是：靠 runner 镜像自带。

### 5.8 `language_backend_guards.rs` 头注释称会拉起 vitest（**已修**）

上一版自己写的注释是错的：它声称「这些用例真的拉起 go / pytest / vitest
子进程」，但文件里**没有任何 vitest 用例**（`grep` 确认 vitest 只出现在
注释里）。13 条实为 go / pytest / mvn。
Node 侧的 3 个缺陷由 `src/build/node.rs` 的解析器单测覆盖，
那些用例只喂字符串、**不起 npm 进程**。注释已改正，
避免后人误以为 Node 有真实工具链 e2e 覆盖。

### 5.9 一个环境假红，差点被我当成代码回归（已排查）

重跑 e2e 时 **6 条 pytest 用例报 `passed=0 failed=0`**。
第一反应是「我改坏了」——但我本轮对该文件只动了注释。

`git stash` 对照确认：**未改代码的基线同样失败**。真因是本机
`python3` 是 mise 3.14 且**未装 pytest**，后端执行 `python3 -m pytest`
直接报错。装上 pytest 9.1.1 后重跑：**13 passed / 0 failed**。

教训：后端测试失败时，**先 `git stash` 做基线对照**，
再怀疑代码；否则会把环境问题写成「回归」。

### 5.10 `sisyphus-bridge` 的 7 个既有 clippy 错误（**已修**）

`git stash` 对照确认它们是基线已有的、与本轮改动无关。
既然本轮就在动这个 crate 的邻近代码，一并清掉：

| lint | 数量 | 处理 |
|------|------|------|
| `field_reassign_with_default` | 2 | 改用结构体字面量 + `..Default::default()` |
| `bool_assert_comparison` | 2 | `assert_eq!(x, true)` → `assert!(x)` |
| `unused_mut` | 1 | `node_a` 只被读取 |

其中 `unused_mut` 那处的旁边本来就有一条注释在解释
「`node_b.events` 需要可变借用，所以 `mut` 不能去」——只是它写在了
`node_b` 头上，把真正多余的 `node_a` 漏掉了。

改的是**真实双节点集群测试**里的断言，不是空跑：
`peer::experience_sync` 3 passed、`cluster_integration` 21 passed，
`-D warnings` 下 clippy 0 错误。

## 6. 提交

按时间倒序（均为本轮）：

| 提交 | 内容 |
|------|------|
| `9d55081` | 查清 SIGABRT 取证失败根因（`ulimit -c = 0`），交付 `make rustc-abort-forensics`；纠正第 5 节同源过度归因 |
| `947f44e` | 纠正 SIGABRT 过度归因（报告第 7.4 节）；补上 `scan-flaky` 缺失的并发限制 |
| `56c712e` | 真实工具链 e2e 接入 CI（新增 `toolchain-e2e` job）；修复长期失败的 `audit-sources` job |
| `6f95d40` | 零用例提示语按后端分流；纠正报告里一处不实陈述 |
| `65e410e` | 修复 Java/Maven 解析漏报并根治测试判定式 |
| `564fa2e` | 新增本状态报告并更新 TODO 计划 |
| `6c3a52e` | 修正 Go/Python/Node 测试守卫漏报与计数错误 |

提交前均通过 pre-commit：`cargo fmt --all --check` / `cargo clippy` / `cargo check-all`。

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

> **归因已查明（初版此处写的是“成因至今未定”，现已过时）**：详见第 5 节。
> 崩溃发生在**链接期 rust-lld**，抛 `EAGAIN`，真因是本机
> `vm.overcommit_memory=2`（严格模式）+ `ratio=80`，
> `Committed_AS` 撞上 `CommitLimit`（实测 21.1 GiB）。
>
> 初版那段推理（「`-j 1` 未覆盖到，因为崩在编译期而非链接期」）
> **本身也是错的**：链接期正是 `CARGO_BUILD_JOBS` 管的范围，
> 所以 `-j 1` **确实管到了**——它降低并行链接数，从而降低提交峰值。
> 它只是无法消除单个链接器自身的占用，故不能**保证**不撞墙。
>
> 也就是说：初版「`-j 1` 不能根除」的**结论碰巧对**，但**理由完全错**。
>
> 排查务必看 `Committed_AS` 而非 `free available`——初版拿
> `available` 充足当“已排除 OOM”的依据，方向就反了。
> `make rustc-abort-forensics`（见 7.5）已把该指标做成实时输出。
>
> 本轮已顺手补上唯一真正缺失的一处：`Makefile` 的 `scan-flaky` 目标及其伴随的
> `cargo test --workspace --lib --no-run` **原先未带** `CARGO_BUILD_JOBS=1`，
> 而它是唯一会同时产出多个重型测试二进制的目标，已加上。
>
> 其余未受限的调用经核查**属有意为之**：`test-ci` 刻意镜像 CI
> （GitHub runner 16 GiB，本就无此限制）；`test-core-quick` / `test-core-min`
> 是单包 lib 测试，只产出一个二进制，不触发并行链接。

### 7.5 SIGABRT —— **成因已查明**（补测时捕获）

见第 5 节：**链接期 rust-lld 撞 `Committed_AS` 上限抛 EAGAIN**，
本机 `vm.overcommit_memory=2` + `ratio=80`，
故 `CommitLimit ≈ RAM × 80%`（实测 21.1 GiB）。

`make rustc-abort-forensics` 已能**按需复现**并实时打印提交内存占限比：

```
=== 前置检查 ===
  overcommit_memory = 2 (0=启发 1=始终 2=严格)
  overcommit_ratio  = 80
  提交内存 Committed_AS = 17777 MiB / CommitLimit 21122 MiB = 84%
=== 循环重编 1 轮 ===
  [ 1/1] commit=19515/21122MiB(92%) ✗ 崩溃复现（第 1 轮，exit=101）
    error: linking with `cc` failed: exit status: 1
```

注意崩溃当轮提交量冲到 **92%**，而成功轮次峰值仅 88%——**相关性直接可见**。

根治方案见 5.4（三选一，**均未实测**）。
**本项目不改系统级 `overcommit_*` 设置**（需 root，且会影响该机器所有用户）。

## 8. 明确不在本次范围

- bridge / sandbox / sandboxd / tests crate（工作区排除，见 `.cargo/config.toml`）
- 其他语言后端之外的生产代码路径
- `TestOutcome` 枚举重构（7.1 的下一阶段）——本次只修展示判定，
  改成 `TestResults` 的新维度需审计全部 6 个构造点与消费方
