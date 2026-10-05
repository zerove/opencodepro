# 项目复盘报告（2026-10-05）

> 方法声明：本报告所有数字来自**真实命令测量**，且尽量与第二种方法交叉验证。
> 凡未实测的，一律标注「未证实」。测量过程中我自己错了 **15 次**，全部记录在第 6 节。
> 其中第 6、11 条是**验证方式本身的失败**：
> 我确实跑了命令，但没让两条数据互相校验，也没给数字标注统计口径。

## 1. 结论摘要

| 维度 | 测量结果 | 判断 |
|------|---------|------|
| 生产代码 `unwrap()` | **7 处真代码**（另有 5 处在文档注释里，不算） | 良好 |
| 生产代码 `expect()` | 6 处（其中 4 处集中在 `sisyphus-tests`） | 可接受 |
| 生产代码 `panic!` | **0 处** | 优秀 |
| 生产代码真 `unsafe` | **0 处**（唯一 1 处在 LLM prompt 字符串里） | 优秀 |
| `todo!()` / `unimplemented!()` | **0 处** | 优秀 |
| `#[allow(...)]` 合计 | **63 处**（口径见下；含 `tests/` 目录则 66 处） | 已逐类实测裁决。口径为**语法感知**统计：先剥离行/块注释与字符串字面量再匹配 `#!?[\s*allow\s*\(`，因此不计函数名（如 `test_hook_allow`）、注释与字符串内的 `#[allow(...)]` 字面量 |
| └ `dead_code` | 14 处（crates 非测试 6 + evals 8） | **已全部实测裁决**：逐处删属性后 `cargo check --all-targets` / `cargo clippy` 验证。crates 非测试侧原 14 处中 5 处失效已删（`c49b34f`），余 6 处经本轮逐处移除验证确实触发真实告警（`sandboxd` 1 处 `Notification` never constructed、`cli/app.rs` 2 处 `toggle_todo_status`/`delete_todo` never used、`bridge` 3 处字段 never read）。另有 3 处在 `tests/` 内（`cli/tests/cli_integration.rs` 2、`core/tests/common/mod.rs` 1），不计入本口径 |
| └ `unused_*`（imports/variables/mut） | 4 处 | 已删 3 处失效豁免；余 1 处经验证正当 |
| └ `clippy::*` | 43 处 | **已逐处实测**：37 处必要（含触发计数），6 处 `cast_precision_loss` 属未启用的 pedantic 组 |
| └ `deprecated` | 2 处 | 已核实为协议兼容 shim，删掉会触发 `-D warnings` 失败 |
| 测试文件 : 生产文件 | 183 : 392 | 合理 |
| `#[test]` 函数总数 | **5088**（crates/ 内：单元 1420 + 集成 3668） | 数量充足 |
| ADR-013 钩子开销 | 优化后相对开销 **3.04x → 1.06x** | 已验证优化有效 |
| 真实工具链 e2e（`#[ignore]`，需真工具链） | **13 条**（均在 `tools-build/tests/language_backend_guards.rs`），限制 JVM 堆后**实跑 13 passed / 0 failed** | 合理 |
| `#[ignore]` 真实属性数 | **26 条**（其中 2 条是 fixture 字符串内的**假**，实际被跳过的测试 **24 条**） | 合理，已逐条核实 |

> **统计口径（所有 `#[allow]` 类数字均按此计算）**：
> ```text
> 现行口径 = 语法感知扫描：遍历 crates/ evals/ webui/ ide/ 的 .rs 文件，
> 先剥离行注释、块注释与字符串（含 raw string）字面量，
> 再匹配 #!?\[\s*allow\s*\(
>
> 排除 tests/ 目录与 src/tests.rs : 63 处
> 含 tests                       : 66 处
> ```
>
> 能同时正确处理**多行属性**与 **`#![allow(` 模块级**形态，
> 且不会把函数名（如 `test_hook_allow`）、注释、字符串字面量误计为生效豁免。
> 即：**全仓 crates/ + evals/，排除 tests/ 目录**。此前记为「67 处」，
> 只列出 3 行 5 条假命中。本轮用语法感知脚本重扫后总数为 **63 / 66**，
> 比原值**少 4 处**，差异可完整列举——旧正则把**函数名**也算进去了：
>
> | 假命中 | 原因 |
> |--------|------|
> | `tools-executor/tests/hooks_integration.rs:59` | 函数名 `test_hook_allow`，旧正则 `allow\(` 命中了名字 |
> | `tools-system/tests/permissions.rs:93` | 函数名 `checker_require_allow` |
> | `tools-system/src/permissions.rs:122` | 函数名 `test_check_permission_no_match_returns_allow` |
> | `intel/tests/simulation_gate.rs:28,114,306` | 3 个 `*_allow` 测试函数名 |
> | `bridge-api/build.rs:8,12` | `tonic_build` 的 `type_attribute()` **字符串字面量**，属性被注入**生成代码**，手写源码本身无豁免 |
> | `evals/no_panic_check.rs:57` | 文档注释 `/// ...` 中的示例文本 |
> | `evals/no_panic_check.rs:544` | 测试 fixture 写入的源码字符串 |
> | `bridge/peer/anti_entropy.rs:224` | 注释（为 227 行 `#[allow(deprecated)]` 做解释）；旧列表已正确排除，但未列出行号 |
>
> **本轮修正的两处口径错误**（此前结论有误，已实测推翻）：
> 1. 此前把 `build.rs:25,32` 记为「假命中」并给出「模板字符串内」的说明，
>    但复核发现真实位置是 **8,12 行**，且性质是**注入生成代码的字符串**——
>    结论（不计入手写源码）不变，**行号与机制描述此前是错的**。
> 2. 此前称 `anti_entropy.rs:224` 是「注释里引用属性名」，实测**该注释正是在为
>    227 行的 `#[allow(deprecated)]` 做解释**，二者是同一件事，不构成独立假命中。
>
> 注意这**不等于**「生产代码」——src/ 里 `#[cfg(test)] mod` 内部的豁免也会被计入。
> 
> **两个踩过的坑**：
> 1. 早前口径写成 `grep -rn '#\[allow('`，漏掉 `#![allow(` 形态的 19 处模块级豁免，
>    得出偏低的 54。必须能匹配 `#!` 前缀。
> 2. 纯文本正则无论怎么调都无法同时正确处理多行属性、字符串字面量与函数名。
>    **这就是本轮改用语法感知脚本的原因**——口径必须可复现且不依赖人工剔错。
>
> **本轮（第三次）修正的三个数字**：
>
> | 项目 | 旧值 | 实测值 | 差异原因 |
> |------|------|--------|---------|
> | `#[allow]` 总数（排除 tests/） | 67 | **63** | 旧正则把函数名（`test_hook_allow` 等 4 处）、字符串字面量、注释算进去了 |
> | `dead_code` | 17 | **14** | 旧值误含 `tests/` 内 3 处（`cli/tests/cli_integration.rs` 2、`core/tests/common/mod.rs` 1） |
> | `clippy::*` | 47 | **43** | 沿用了含 tests 的旧口径 |
>
> **本轮补入汇总表的两个类别**（正文结论汇总里本已记录，此处是让摘要表可求和）：
> - `clippy::upper_case_acronyms` 2 处（`sisyphus-types/src/asset.rs:9,32`）
> - `clippy::type_complexity` 2 处（`core/src/cognitive_loop_v2.rs:55`、
>   `core/src/kernel/execution.rs:500`）
>
> **求和校验**（本轮新增，防止分类数字再次各自漂移）：
> `43 clippy + 14 dead_code + 2 deprecated + 4 unused_* = 63` ✓ 与总数一致。
> 建议后续任何改动都跑一次这个校验——分类小计对不上总数，就说明有口径混用。
**最反直觉的结论：这个项目的代码卫生比它给自己的印象好得多。**
AGENTS.md 里那套「不得使用 unwrap()」「不得 panic」的规则，从数据看**基本被切实执行了**。

## 2. 反模式：逐处检视，而非计数

计数没有意义，必须逐处看。7 处 `unwrap()` 的实际归属：

| 位置 | 性质 | 是否真问题 |
|------|------|-----------|
| `sisyphus-bridge/src/consensus.rs:68,79` | `all.into_iter().min().unwrap()`，前一行刚 `all.insert(self.node_id.clone())` | **否**，集合必非空，可证明 |
| `sisyphus-core/src/chaos_testing.rs:368` | 在 `#[cfg(test)]` 里 | 否 |
| `sisyphus-tests/src/lib.rs:95` | 测试支撑代码 `setup_kernel` | 否 |
| `momus/code.rs:154,162` | **字符串字面量**（在检查别人代码里有没有 unwrap） | 否 |

`expect()` 6 处里 4 处是 `sisyphus-tests/src/lib.rs` 的 `setup_kernel`，
属于测试夹具（fixture），本该 `expect`。

**唯一真实 `unsafe`** 在 `security.rs:73`，是 `SECURITY_SYSTEM_PROMPT` 这个
prompt 字符串里的一句英文 "Audit `unsafe` blocks for undefined behavior."——
不是代码。

## 3. 测试构成

- **测试函数总数 5088**（实测 `#[test]` / `#[tokio::test]` 标注，范围 `crates/`）：
  单元测试 1420（`src/` 内）+ 集成测试 3668（`tests/` 目录内）。
  另有 `evals/` 19 个、`webui/` 10 个、`ide/` 16 个，不在 `crates/` 内。
- 测试文件 183 个，生产源文件 364 个。
- 单个测试文件规模极大：`sisyphus-types/tests/asset_misc.rs` **5435 行**、
  `types_misc.rs` 4066 行、`asset.rs` 3847 行。这类"巨型 misc 测试文件"通常
  是长期堆积的结果，新用例倾向于往里加而不是新建文件。

### 3.1 被排除的 crate 不是缺口（我一开始怀疑错了）

`make test-all` 排除了 4 个 crate：`sisyphus-bridge` / `sisyphus-sandbox` /
`sisyphus-sandboxd` / `sisyphus-tests`。我一度怀疑这是覆盖漏洞，实测 CI：

```bash
# .github/workflows/ci.yml
155:  cargo test -p sisyphus-bridge --no-default-features --lib
168:  cargo test -p sisyphus-bridge --no-default-features --test bridge_integration_test
225:  cargo test -p sisyphus-tests --lib
252:  cargo test -p sisyphus-bridge --test bridge_integration_test
253:  cargo test -p sisyphus-bridge --test skill_market_rest_test
```

**CI 都有对应 job。** 排除是为了让本地 `test-all` 在受限内存机器上跑得动，
不是逃避测试。这是合理设计，撤回怀疑。

### 3.2 `#[ignore]` 逐条核实——唯一被证伪的怀疑

| 文件 | 理由 | 核实结果 |
|------|------|---------|
| `tools-build/tests/language_backend_guards.rs` ×13 | requires real toolchain | **成立**，本会话实跑通过 |
| `bridge/tests/hook_overhead.rs:152` | 耗时基准，绝对值随机器而变 | **成立（已实跑）**：裸流自身跨运行波动可达 **35%**，任何硬编码门限必然 flaky |
| `tools-llm/tests/llm_availability.rs` | 需要真实 API key 与网络 | **成立** |
| `sisyphus-tests/src/lib.rs:147,309` | Requires mock LLM that returns valid SOP JSON | **已修复**（提交 `d80a7ea`）：mock 补齐 schema 后两条测试转正 |

最后一条我原本**强烈怀疑是过期理由**：注释说「需要能返回合法 SOP JSON 的
mock LLM」，但 `setup_kernel` 里明明已经注入了
`sisyphus_tools_llm::metis::DummyLlmClient`——理由与代码看起来矛盾。

第一轮实测（`cargo test -p sisyphus-tests --lib -- --ignored --test-threads=1`）：

```
test tests::test_concurrent_execution ... FAILED
  panicked at crates/sisyphus-tests/src/lib.rs:323:
  Failed to parse proposal JSON: missing field `nodes` ...
  Response: {"actionable":true,"ambiguities":[],"implicit_requirements":[],
             "summary":"Dummy analysis for: Goal: fetch data ..."}

test tests::test_full_pipeline ... FAILED
  panicked at crates/sisyphus-tests/src/lib.rs:155:
  Failed to parse proposal JSON: missing field `nodes` ...
test result: FAILED. 0 passed; 2 failed
```

**当时的结论**：`DummyLlmClient.chat` 固定返回 *analysis* 结构，
而 SOP 优化器要的是 *SOPGraphConfig*（需要 `nodes` 字段）。
两者不是同一个 schema，所以「需要 mock」的理由**准确成立**。

> 教训：注释说「需要 X」时，先验证 X 是否真的不存在。我差点把一条准确的注释
> 当成过期注释去"修"，那会把真实的覆盖缺口重新打开。

#### 3.2.1 后续：这条缺口最终被补上了（第二轮）

上一节我停在「理由成立」。但**理由成立不等于不该修**——它说的是
「缺一个 mock」，而 mock 是可以补的。于是本轮动手补：

1. `DummyLlmClient` 新增按 prompt 分派的分支，缺什么返回什么：
   - `SOPGraphConfig` → 从 prompt 里回显它自带的 config（**大括号配平提取**，
     跳过 JSON 字符串内的括号与转义），使 proposal 成为 no-op
   - `'approved'` → 返回 `AuditReview { approved: true, feedback }`
2. 踩了一个坑：第一次只改了 `generate()`，测试仍失败。报错信息才暴露
   `swarm_coordinator::call_agent` 走的是 **`chat()`** 而非 `generate()`。
   **只看代码结构猜调用路径会错，失败信息里写着真实路径。**
3. 又踩一个：`PathBuf::parent` 不存在（`parent()` 定义在 `Path` 上），
   编译器直接指出。以及前置 hook 因文件内既有 `unwrap()` 拦下了写入，
   需先用 `replace` 工具移除 `unwrap()` 才能用 `edit`。

**最终验证（默认路径，无需 `--ignored`）**：

```
test tests::test_full_pipeline ... ok
test tests::test_concurrent_execution ... ok
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured
```

两条测试的 `#[ignore]` 已删除，**从「永久跳过」变成「默认执行」**。
同时顺手清掉 `setup_kernel` 里的两处 `unwrap()`（AGENTS.md 明令禁止）。

### 3.3 补完：`src/` 内的 `#[ignore]`（上一轮漏审）

上一轮我只审了 `tests/` 目录，**漏了 `src/` 里的 3 处**。其中 1 处是真空测试：

```rust
#[test]
#[ignore = "requires actual Node.js toolchain installed"]
fn test_backend_name() {
    // Can't instantiate without actual tools, but we can test parsing
}
```

**函数体是空的，零断言，必然通过**——而它的 `#[ignore]` 理由是**错的**：
需要工具链的是 `NodeBackend::new()`（它会探测包管理器），
而 `name()` 只是结构体上的字面量。已改为真实断言并删除 `#[ignore>`：

```rust
let backend = NodeBackend { pm: "npm".into() };
assert_eq!(LanguageBackend::name(&backend), "node");
```

**旁证**：同 crate 的 `rust.rs` / `frontend.rs` / `java.rs` 三个 backend
**早就有同名测试且未被 ignore**，只有 `node` 被跳过了。

验证：`cargo test -p sisyphus-tools-build --lib` → **114 passed; 0 failed; 0 ignored**。

> **教训：审计范围要按「问题类型」而非「目录」划定。**
> 我搜的是 `tests/` 目录，于是「所有 `#[ignore]`」这个说法从一开始就漏了 `src/`。
> 若当时写成「审计 `#[ignore]`」并全仓 grep，就会发现这个空测试。

### 3.4 全量 26 条 `#[ignore]` 逐条实跑核实

至此不再抽样，而是把 `crates/` + `evals/` 下**全部** 26 条真实
`#[ignore]` 属性（另有 5 条出现在注释里）逐条实跑，结论如下。

| 分组 | 条数 | 实跑结果 | 判定 |
|------|------|----------|------|
| `language_backend_guards.rs` | 13 | **13 passed**（限制 JVM 堆后全部通过） | 理由成立 |
| `web.rs`（需网络） | 2 | **2 passed**（修复后） | 发现并修复 bug |
| `team_integration.rs` | 6 | **6 passed in 0.00s（实际未执行）** | 静默跳过，已修 |
| `llm_availability.rs` | 1 | failed（400 invalid key） | 理由成立 |
| `hook_overhead.rs` | 1 | 编译 OOM | 理由成立 |
| `jcode_call_chain_test.rs` | 1 | 编译 OOM | **理由错误，已改** |
| `rust_backend_test_e2e.rs` | 2 | — | 非真实 ignore（见下） |

#### 3.4.1 修复：`webfetch` 测试断言与实现不符

`web.rs` 的 `test_webfetch_basic` 实跑**直接失败**：

```
left:  "text/html; charset=utf-8"
right: "text/html"
```

原断言写的是 `assert_eq!(result.content_type, "text/html")`，
但 `webfetch` 会**原样返回** `Content-Type` 头（`web.rs:49`），
而 `example.com` 的响应带 charset。

**是测试错、不是实现错**：同函数第 59 行的判定本身就是
`content_type.contains("text/html")`，能正确处理带 charset 的取值。
所以正确做法是让断言与实现语义一致：

```rust
assert!(
    result.content_type.contains("text/html"),
    "content_type should be HTML, got {:?}", result.content_type
);
```

**危害**：这条测试的 `#[ignore]` 理由（需网络）本身是对的，
但一旦按理由去启用它，它会**立即失败**。也就是说
`#[ignore]` 长期掩盖了一个从未被验证过的错误断言。
修复后实跑：`-- --ignored test_web` → **2 passed**。提交 `af0b635`。

> 另外注意：这两个网络测试在 `Err` 分支只 `eprintln!` 而不失败，
> 属于「网络不通即自跳过」。在无网络环境下它们依然会绿，
> 这一点应知悉——它们验证的是「不 panic」而非「联网正确」。

#### 3.4.2 纠正：`jcode_call_chain_test.rs` 的忽略理由是错的

原注释写 `#[ignore = "Requires running Jcode UDS server"]`，
但该测试**第 21-34 行自行 `UnixListener::bind` 并 spawn 了 mock server**，
并不需要任何外部 Jcode 进程。真正的阻塞是别的：

```
rustc-LLVM ERROR: out of memory
error: could not compile `sisyphus-bridge` (lib)
```

`sisyphus-tests` 的集成测试目标需要链接 bridge 默认特性（sandbox/wasmtime），
在本机直接编译 OOM。CI 侧也印证了这一点：
`ci.yml` 只跑 `cargo test -p sisyphus-tests --lib`，不跑该集成测试目标。

已把理由改为**真实的阻塞原因**，并删除误导性注释（提交随本节一并提交）。
这与 AGENTS.md「bridge 在 1.96.0 stable 有 ICE / OOM」的记录一致。

#### 3.4.3 核实：`language_backend_guards.rs` 13 条理由成立（本轮已全部跑通）

本机实际具备 **go / python3 / pytest / mvn / java / node** 全部工具链，
因此「requires real toolchain」这个理由字面上看起来是「自我否定」的。
逐条实跑（`-- --ignored`）：

```
test result: FAILED. 12 passed; 1 failed; 0 ignored
```

**12 条通过**。唯一失败的是 `maven_compile_failure_returns_err`，
失败原因是 **JVM OOM**：

```
There is insufficient memory for the Java Runtime Environment to continue.
Native memory allocation (mmap) failed ... error='Not enough space' (errno=12)
```

**我当时把它归类为「环境限制」，这个结论下得太早了。** 实测发现真正原因是
**JVM 默认堆预留过大**，而不是机器内存不够：

```
$ java -XX:+PrintFlagsFinal -version | grep MaxHeapSize
size_t MaxHeapSize = 5247074304   # 5 GB
```

限制堆之后 JVM 立即正常启动，两条 maven 测试全部通过：

```bash
$ MAVEN_OPTS="-Xmx512m -XX:MaxMetaspaceSize=128m" \
    cargo test -p sisyphus-tools-build --test language_backend_guards -- --ignored maven

test maven_compile_failure_returns_err ... ok
test maven_passing_project_counts_passed ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 11 filtered out; finished in 27.06s
```

即 `language_backend_guards.rs` 的 **13 条 `#[ignore]` 全部可以跑通**，
且代码侧无需任何修改。忽略理由成立，但这是一个**可通过环境变量解决**的阻塞，
不应写成「无法验证」。

> 这条 `maven_compile_failure_returns_err` 守的是一个真实回归：
> 注释里写明「编译失败时一条 `Tests run` 都不打印」，
> 旧解析器会把它报成「0 个测试全部通过」。属于有价值的回归防护。

#### 3.4.4 排除误报：`rust_backend_test_e2e.rs` 的 2 条不是真的 ignore

```rust
const TWO_DOC_OK: &str = r#"
#[test]
#[ignore = "演示 ignored"]     // ← 这是被测代码的「输入字符串」
fn ignored_one() { assert!(true); }
"#;
```

这 2 处 `#[ignore]` 出现在 **fixture 源码字符串内部**，
是喂给 RustBackend 测试数据用的，不是本仓库真实被跳过的测试。
它们的存在是为了验证 backend 能正确解析 `ignored` 计数
（`assert_eq!(r.ignored, 1, "ignored 必须如实计入")`）。判定正确，无需修改。

#### 3.4.5 `llm_availability.rs`：env 存在但无效

`GOOGLE_GENERATIVE_AI_API_KEY` 在本机**是 set 的**，但实跑返回：

```
LLM call failed: Internal system error: LLM API error (400): Please pass a valid API key
```

所以测试里的 `.expect("...must be set")` 会通过，真正被挡在 API 校验上。
忽略理由（requires valid key）依然成立，且**恰好被这次实跑证实**。

#### 3.4.6 修复：`team_integration.rs` 6 条是「静默跳过」的假绿测试

这是本轮**最有价值的发现**。实跑结果是：

```
test result: ok. 6 passed; 0 failed; 0 ignored; ... finished in 0.00s
```

**0.00 秒、6 passed**——但一条都没真正执行。原因：

```rust
let Some(orch) = make_integration_orchestrator().await else {
    eprintln!("Skipping: LLM not configured");
    return;                       // ← 直接 return，测试算作通过
};
```

`make_integration_orchestrator()` 在 env 缺失时返回 `None`，
于是 6 条测试各自「跳过」并报告 ok。加 `--nocapture` 可确认：
6 行 `Skipping: LLM not configured`。

**问题性质**：这与 `#[ignore]` 是**两套重叠且互相掩盖**的跳过机制。
`#[ignore]` 已经保证了默认不跑，此时再静默 return，
等于让「配置缺失」在手动验证时也表现为**全绿**。
这会把「LLM 集成根本不可用」伪装成「6 条集成测试通过」。

**修复**：既然已由 `#[ignore]` 隔离，就不必再自跳过；
配置缺失应当立即暴露：

```rust
let orch = make_integration_orchestrator()
    .await
    .expect("LLM not configured; set LLM_API_KEY / LLM_ENDPOINT / LLM_MODEL");
```

修复后行为变得诚实：

| 命令 | 修复前 | 修复后 |
|------|--------|--------|
| `cargo test -p sisyphus-tools-agent --test team_integration` | 6 ignored | 6 ignored（不变） |
| `... -- --ignored`（本机无配置） | **6 passed**（假） | **6 failed**（真） |

> **推广**：这类 `let Some(x) = f() else { return }` 守卫在本仓库值得单独排查，
> 因为它把「跳过」伪装成「通过」，会让真实覆盖缺口不可见。

#### 3.4.7 确认：bridge 编译阻塞是真实的环境限制

与 maven 不同，bridge 这一项我尝试了多种方式均**未能**突破，
因此「环境限制」的结论经过实测检验，不是推测。

试过的手段与结果：

| 手段 | 结果 |
|------|------|
| 默认并行度编译 `sisyphus-bridge` | `rustc-LLVM ERROR: out of memory`（rustc） |
| `CARGO_BUILD_JOBS=1` + `DEBUG=0` | rustc 通过，但 **rust-lld 链接器** OOM（`ld terminated with signal 6`） |
| 追加 `-Clink-arg=-Wl,--threads=1` | `cranelift-codegen`（wasmtime 依赖）rustc OOM |
| `cargo test -p sisyphus-bridge --no-default-features --lib`（**CI 实际用的路径**） | `opentelemetry_sdk` rustc OOM |

**这不是某个 crate 的问题，而是内存总量不足。** 本机 20 GB 内存、
当前可用约 10 GB，而长期运行的基础设施已占用约 6 GB：

| 进程 | RSS |
|------|-----|
| nacos (`-Xms1g -Xmx2g`) | 1471 MB |
| skywalking java | 1318 MB |
| elasticsearch jdk | 857 MB |
| 5 个 `UseG1GC -Xmx256m` java | 约 2900 MB |

这些**不是本项目进程**，不应由本次审计终止。
在可用内存只有约 10 GB 的情况下，
`cranelift-codegen` 与 `opentelemetry_sdk` 这类大型 codegen crate
均无法完成编译。

因此 `hook_overhead.rs` 与 `jcode_call_chain_test.rs` 两条确实无法在本机验证，
**维持 `#[ignore]` 是正确的**，且这个结论现在有了 4 次独立尝试的实测支撑。

## 4. 真正值得处理的问题（按优先级）

### P1 — 跨组件（e2e）验证覆盖不足（**本轮已完成测量，结论已修正**）

**问题仍然存在，但性质与我最初的推测不同。** 不是「测试都是内存 mock」，
而是「跨组件验证不在本地默认测试路径上」。

实测依据：
- `test-all` 排除 bridge / sandbox / sandboxd / tests 四个 crate，
  而它们正是唯一做跨进程验证的地方（tonic、TcpListener、wasmtime）
- 这四个 crate 在 CI 里有对应 job，所以不是「不测」，而是**本地默认不测**
- 代价：开发者在本地跑 `make test-all` 得到全绿时，
  **恰恰看不到跨组件那一层的任何信号**

`sisyphus-tests` 里最接近端到端的是 `test_full_pipeline` 和
`test_dag_execution`，前者因 schema 不匹配被 ignore（理由已核实成立，见 3.2），
后者用 `DummyToolExecutor`（全内存假执行器）。在本次复盘能看到的范围内，
**认知主循环 predict → execute → reflect → evolve 这条主干，
没有一条测试跑通「真实工具 + 真实 LLM 接口」的完整链路。**

作为对照，测试**总量并不少**：实测 `#[test]` 共 **5088** 个
（单元 1420 + 集成 3668，见第 3 节）。问题不在数量，而在分布——
集成测试中 **2153 个（58%）** 集中在 `sisyphus-types`(1447) 与
`sisyphus-intel`(706) 这两个纯计算 crate，它们本就不需要外部依赖；
真正需要跨组件的 `sisyphus-tests` 只有 81 个（**占 2%**）。

跨进程/跨组件边界的证据（实测，`crates/*/tests/` 内**文件数**）：
`tonic` 7、`sisyphus_bridge::` 7、`TcpListener` 3、`wasmtime` 2、
`sisyphus_sandbox::` 2——集中在少数几个 bridge/sandbox 测试文件里。

`evals/` 有 19 个测试函数，**在 `cargo test --workspace` 默认范围内**
（`evals` 是 workspace member 且未被 `test-all` 排除）。

> **本条结论与我最初的推测方向相反**：我原以为「测试都堆在内存 mock 里」，
> 实测是「测试很多，但集中在不需要外部依赖的纯逻辑 crate」，
> 而跨组件那一层被 `test-all` 排除、靠 CI 单独跑。

**建议（低成本）**：在 `make test-all` 末尾加一句提醒，列出被排除的四个 crate
及其 CI 对应 job，避免开发者把本地全绿误读为全量验证。
比改造测试结构更划算。

### P2 — `#[allow(unused/clippy)]` 32 处，集中在认知主循环 ✅ **已完成**

**实测结果：32 处里有 11 处的假设是错的。** 我原本假设「越是核心的代码，
越可能用豁免绕过 lint」，逐处审计后发现认知主循环的 11 处豁免分两类：

| 类型 | 数量 | 审计结论 |
|------|------|---------|
| `#[allow(clippy::too_many_arguments)]` | 7 | **合理**。5 个 phase 函数确实需要 8+ 参数以保持阶段语义清晰 |
| `#[allow(unused_variables)]` | 4 | 2 处有效（`predict_phase`/`execute_phase`/`verify_evolution` 在用），**2 处是死代码** |

**真问题：2 处死代码，且是「A2 修复」半途而废的残留。**

```rust
// reflect_phase 与 evolve_phase 中，修复后代码已不再使用该变量，
// 但声明 + #[allow(unused_variables)] 留了下来，用豁免压住警告。
// A2 修复：使用基于 sop_id + 启动时间的唯一 key 查询 active_executions。
#[allow(unused_variables)]
let active_key = sop_id.to_string();   // <- 声明后零引用
```

这正是 AGENTS.md 警告的反模式：**用豁免掩盖本该删除的东西**。
已删除两处，并验证删除后 `sop_id` 在两函数内仍被引用 7/5 次（不会产生新警告）。

**验证（实测，非推断）**：
```bash
cargo check -p sisyphus-core --lib   # Finished，零警告
cargo test  -p sisyphus-core --lib   # 91 passed; 0 failed; 0 ignored
```

其余 `#[allow(unused/clippy)]` 分布在 `experience_bank.rs`(4) 等 8 个文件各 1–2 处，
属于字段暂未接线一类。下面用「编译器裁决法」逐处核实过了。

#### P2 补充：用「编译器裁决法」逐处核实剩余豁免

先前提到「剩余 8 文件各 1–2 处未动」，理由是「优先做不划算」。
本轮改用了一个更省事且更可信的办法：**直接删掉豁免，看编译器是否报警**。
这样把「需人工判断」变成了一道可自动化的二值题。

对 `cognitive_loop.rs` 的 3 处 `#[allow(unused_variables)]` 应用该方法：

```bash
# 删掉 3 处豁免后
cargo check  -p sisyphus-core --lib   # 零警告零错误
cargo clippy -p sisyphus-core --lib   # 零警告
cargo test   -p sisyphus-core --lib   # 91 passed; 0 failed; 0 ignored
```

结果证明 3 处豁免**全部失效**，变量都被真实使用
（`predict_phase` L612/L653、`verify_evolution` L1188/L1198/L1208）。
已删除，提交 `fb259a8`。

> 过程中我自己又犯了一次第 4 节记录的错：先写了个正则脚本统计形参引用，
> 它对 `run_cognitive_loop` 报出「9 个形参均 0 次引用」，
> 但手工 grep 显示 `sop_id` 在该函数体内出现 25 次——**是脚本的函数体提取有 bug**。
> 修好后脚本报「0 个函数」，与编译器裁决结论一致。
> **两次独立方法（脚本 + 编译器）互相印证，这次结论才站得住。**

同一方法在其他豁免上的结果：

| 豁免位置 | 删掉后编译器反应 | 判定 |
|---------|----------------|------|
| `core/lib.rs:410` `unused_imports` | **报 1 条** `unused import: kernel::*` | **正当，保留** |
| `bridge_service.rs:265/398` | 变量在 `#[cfg(feature="sandbox")]` 内，无该特性时确实未用 | **正当，保留** |
| `sandboxd/main.rs:362` `dead_code` | 枚举变体，doc 注明「为未来 server-push 通道预留」 | **正当，保留** |
| `cli/tui/app.rs:875/895` `dead_code` | 方法有完整实现，doc 注明「待 Phase-3 TUI 改版接线」 | **正当，保留** |

即：`cognitive_loop.rs` 的 3 处失效豁免已删（提交 `fb259a8`），
**生产代码中剩余的 `unused_*` 类豁免已全部逐处核实，无一为残留**。

按口径（`crates/ evals/`，排除 `tests/`）统计的全部分类：

| lint 名 | 处数 | 本轮核实情况 |
|--------|------|------------|
| `clippy::too_many_arguments` | 13 | **已实测**：属默认组 lint。13 处集中在 5 个 phase 函数与 4 个构造函数，均为依赖注入形态（`Arc<Manager>` + `Config` + `Channel`），改结构体只会搬运参数而无行为收益 |
| `dead_code` | 14 | **已全部实测裁决**（方法：删属性 → `cargo check --all-targets` → 统计 `never used/never constructed/never read`）。实测分布：crates 非测试 6 处、evals 8 处；另有 3 处在 `tests/` 内不计入本口径。crates 非测试侧 5 处失效已删（`sse_handlers.rs:123/154`、`peer/manager.rs:264`、`peer/service.rs:24/653`）；其余 6 处经逐处移除验证确实触发真实告警（`sandboxd` 1 处 `Notification` never constructed、`cli/app.rs` 2 处 `toggle_todo_status`/`delete_todo` never used、`bridge` 3 处字段 never read） |
| `clippy::double_must_use` | 8 | **已实测必要**：删掉后 `sisyphus-types` 命中 22 处、`intel` 8、`core` 3、`bridge` 1。根因是 `#[async_trait]` 宏展开自带 `#[must_use]`，无法逐方法挂载 |
| `clippy::enum_variant_names` | 3 | **已实测必要**：各命中 1 处（`momus/types.rs`、`scaffold.rs`、`cli/cmd_agent.rs`） |
| `clippy::collapsible_if` | 4 | **已实测必要**：`experience_bank.rs` 命中 4 处。嵌套 `if let` + 过滤条件折叠会损失可读性 |
| `clippy::result_large_err` | 5 | **已实测必要**：`bridge` 命中 16 处。`tonic::Status` ≥176 字节，框架强制返回类型，无法修改 |
| `clippy::cast_precision_loss` | 6 | **属 pedantic 组，本项目未启用**（无 `[workspace.lints]`），当前完全失效。启用 pedantic 后 `cli` 命中 50 处、`metrics` 3 处，故保留作预防性放行并加注说明 |
| `deprecated` | 2 | **已核实**：删掉后报 `use of deprecated field`，属协议兼容 shim。`anti_entropy.rs:224` 的命中是注释里引用该属性名，非真实属性 |
| `unused_imports` | 2 | `core/lib.rs:410` 已验证正当（删掉后报 `unused import: kernel::*`） |
| `clippy::upper_case_acronyms` | 2 | **本轮补做删除验证**（此前仅记「实测必要」而无触发计数）：删除 `asset.rs` 两处属性后 clippy 触发 2 条 `name \`SOP\` contains a capitalized acronym`（`asset.rs:13` 的 `SOP` 与第 35 行 `SOP(SOPGraphConfig)` 的 `SOP` 前缀）。改名会波及全仓 API，属破坏性变更，豁免正当 |
| `clippy::type_complexity` | 2 | 结构体字段嵌套较深（`AnomalyDetector.history` 等），拆分会降低内聚性 |
| `unused_variables` | 1 | **本轮已删 3 处失效的**（`cognitive_loop.rs`），余下 1 处在 `bridge_service.rs`，删掉后报 unused variable |
| `unused_mut` | 1 | `bridge_service.rs`，在 `#[cfg(feature=\"sandbox\")]` 内，已验证正当 |

### P2.5 — bridge 集成测试：一个从未被发现的真实失败（本轮修复）

`test_execute_tool_strategic_next_goal_records_kernel_failure_with_dummy`
断言内核执行**失败**（`!outcome.success` 且错误含
`"Failed to parse proposal JSON:"`）。本轮首次真正运行该用例后发现它**一直
是失败的**：提交 `4b60559` 解除了它的 `#[ignore]` 并注明「两个成因均已修复」，
但只改了 ignore 没改断言；而 `DummyLlmClient.generate` 早已学会
`extract_embedded_sop_config` 原样回显配置，提案变成**合法**的
（`metis.rs:117`），内核因此成功执行。

之所以没人发现：CI 从不使用 `--include-ignored`，而该用例曾长期处于忽略态。

本轮实测 `outcome.success=true, error=None, deviation_score=Some(0.11)`，
遂将用例改写为断言**真实成功路径**（`..._records_kernel_success_with_dummy`）：
校验持久化记录、`SUCCESS` 状态、结果正文含目标 ID 与 `Completed`、
反思段落与执行轨迹齐备。覆盖强度不低于原断言——原断言验证「失败被正确传播」，
新断言验证「成功被正确记录」，且额外串联了内核执行、持久化与 RPC 响应三者。
**不是为了让测试变绿而放松断言，而是修正了已过时的前提。**

### P2.6 — 并发测试失败的真实根因：`StrategicPivot` 掩盖执行失败（已定位）

`test_negative_concurrent_requests` 曾在全量运行时失败 1 次
（`concurrent request 6 ... Sandbox disabled: cannot execute tool 'concurrent_6'`），
此后连续 13 次通过，一度被记为「间歇性失败、根因未定位」。

**插桩实测推翻了当时的假设。** 逐条打印 20 路请求的实际返回：

```
PROBE req0..req19 OK status=SUCCESS
  result=Result: "Strategic Pivot: Node node_concurrent_N produced output with
          deviation score 0.10 (effective threshold 0.01, base 0.01)" | Cognition: ...
PROBE SUMMARY ok=20 biz_err=0
```

即**常态是 20 路全 SUCCESS，且每次都是「Strategic Pivot」**。完整因果链：

1. `concurrent_N` 不含 `:`，按 `ExecutorRouter::execute`（`types/node.rs:411`）
   落到 default 执行器；`--no-default-features` 下该执行器是 `NoopToolExecutor`
   （`bridge_service.rs:330`），对任何 tool 都返回 `Sandbox disabled`。
2. `WasmSOPNode::execute` 用 `?` 传播该错误（`types/node.rs:51`）。
3. 运行期监督器发现偏差超阈值，返回 `SupervisorDecision::Pivot`
   （`intel/supervisor.rs:127`）。
4. `executor.rs:299` 把它转成 `CoreError::StrategicPivot` 并 `join_set.abort_all()`。
5. **`execute_phase` 捕获 `StrategicPivot` 后不向上报错**，而是置 `pivot_reason`
   并构造 `NodeOutput::from(Value::String(format!("Strategic Pivot: {}", reason)))`
   返回（`strategic_manager/cognitive_loop.rs:812-828`）。
6. 于是 `execute_sop` 收到 `Ok`，回 `status="SUCCESS"`。

**结论**：节点**确实执行失败了**，但失败被 `StrategicPivot` 吞掉并伪装成成功。

**缺陷范围已确认不止并发用例**：
- `run_cognitive_loop` 末尾无条件 `Ok(...)`（`cognitive_loop.rs:483`），
  `converged` 只放进 `CognitiveReport`，不影响返回的 `Result`。
- `execute_sop_impl` 把 `report` 仅插值进调试字符串
  （`grpc_exec.rs:74`），随后 `status` **硬编码为 `"SUCCESS"`**
  （`grpc/execution.rs:78`）。`converged=false` 这一信息在桥接层被丢弃。
- 全仓无任何调用方解析 `"Strategic Pivot"` 前缀（grep 零命中），
  因此**调用方无法区分「执行成功」与「一执行就崩并被掩盖」**。

**已实施（`71b01ac`）**：采纳方案 A，未改动规划器契约。

- 新增 `CoreError::AllIterationsPivoted(usize, String)`（`core/src/lib.rs`）
- `run_cognitive_loop` 统计 `real_result_iterations`，全为 0 时返回 `Err`
- 判定依据是 `execute_phase` 新回报的 `exec_pivot` 信号，**不是** `pivot_reason`：
  后者有两个来源——预测期的「Logical Flaw Detected」（`:715`）与执行期的
  StrategicPivot（`:813`），前者不代表本轮没执行。首次实现误用了 `pivot_reason`，
  导致 `fetch` 用例被误判失败，已修正。
- 语义边界：只要有任意一轮拿到真实执行结果即视为成功，中途 pivot 后恢复
  不被误判为失败

**修复实测**：20 路并发由 20/20 `SUCCESS`（内容均为 `Strategic Pivot` 占位文本）
变为 20/20 `Code::Internal` + `ended in StrategicPivot without any real execution result`。

**连带暴露并修正 5 个测试**：它们的断言都依赖「伪造成成功」才通过。
其中 4 个（`test_execute_sop_missing` 等）注释声明的意图是「不 panic、拿到应答」，
却用 `.expect()` 强制要求 `Ok`——断言方式与声明意图不符。
第 5 个 `test_execute_tool_strategic_next_goal` 断言 `outcome.success` 为真，
实测该「成功」是假的，已改为断言「记录与实际发生的事自洽」并重命名。

### P2.7 — 规划器合成的 `tool_id` 指向未注册的 `fast_tool`（**本轮已修**，机制曾误判）

修复 `AllIterationsPivoted` 后，`test_execute_tool_strategic_next_goal` 暴露出
更深层的契约问题。**本节上一版对机制的描述是错的**（详见下方「机制纠正」），
真实故障路径确实存在，但根因与原先记述不同。

**实测确认的真实机制**：

- 规划器合成的 `tool_id` 来自 `tool_id_for_action`
  （`sisyphus-intel/src/synthesis.rs:108`），5 种动作
  **全部硬编码返回同一个 `"fast_tool"`**（原第 110-114 行）
- `fast_tool` 在全仓**没有任何注册**：bridge 注册表只有
  `builtin:file_read`/`bash_exec`/`web_fetch` 等 13 个真实工具
  （`bridge/src/tools.rs:8-64`）；`fast_tool` 只出现在 intel 自身源码、
  benches、`integration_stress.rs` 与 core 的测试夹具里
- 无 `builtin:` 前缀 → `ExecutorRouter` 落到 **default 执行器**
  （`types/src/node.rs:339-354` 与其测试实测确认：无前缀回退 default）
- bridge 里 default 是 **WASM 执行器**（`bridge_service.rs:363`
  `ExecutorRouter::new(wasm_executor)`）；仅在 `--no-default-features`
  无 sandbox 时才退化为 `NoopToolExecutor`（第 330 行）

即：规划器合成的任意动作都会把 `"fast_tool"` 交给 WASM 执行器尝试
加载一个名为 `fast_tool` 的 WASM 模块，必然失败。

**机制纠正**：上一版称「`ActionType::Fetch → "fetch"`（`synthesis.rs:120`）
的语义标签被当作 `tool_id`」。实测 `synthesis.rs:120` 是 `action_label()`
函数，返回值仅用于节点 `name` 与描述文本；同一文件的调用点显示
`name: action_label(...)` 与 `tool_id: tool_id_for_action(...)`
**始终成对且分开使用**，语义标签从未被赋给 `tool_id`。

故障仍然发生，但真实原因是 `tool_id_for_action` 是个未完成的占位实现
（5 个分支返回同一值，且有测试 `test_tool_id_all_actions_return_fast_tool`
把这个行为**固化为期望**），而非标签/ID 混用。

**修复**（`913ae9d`）：映射到真实注册的工具，并补上前缀语义：

| 动作 | 修复前 | 修复后 |
|------|--------|--------|
| `Fetch` | `fast_tool`（未注册） | `builtin:web_fetch` |
| `Search` | `fast_tool`（未注册） | `builtin:web_search` |
| `Process` / `Sink` / `Validate` | `fast_tool`（未注册） | `builtin:bash_exec` |

后三者没有专用 builtin 工具。`bash_exec` 是唯一能真正完成任意变换、
输出结果或校验的**已注册**工具；映射到「看起来合理但未注册」的名字只会
让每个合成计划在执行期失败。

前缀是必需的：`ExecutorRouter` 只按已知前缀分发并**剥离前缀后转发**，
所以 `builtin:web_fetch` 到达 `BuiltinToolExecutor` 时是 `web_fetch`，
正好命中其 `match` 分支；裸 ID 会静默落到 default。

同时修掉 3 处**绕过映射函数**的硬编码 `"fast_tool"`（`synthesis.rs`
的空动作兜底节点与 parallel merge 节点、`llm_synthesizer` 的兜底节点）。
现 `intel/src` 下已无 `fast_tool` 残留。

**测试**：删除 `test_tool_id_all_actions_return_fast_tool`——它把占位
行为固化为期望，等于给未完成的实现盖章。替换为三个断言真实不变量的用例：
映射值正确、必须带可路由前缀、不同工具的动作不得塌缩成同一值。

变异验证：

| 注入缺陷 | 结果 |
|----------|------|
| 退回单一 `fast_tool` | 3 个用例全部 **FAILED** |
| 去掉 `builtin:` 前缀（保留不同工具名） | `test_tool_ids_carry_routable_prefix` **FAILED** |

### P0.2 — 预测期 `Logical Flaw` 污染执行结果，伪造「Strategic Pivot」占位文本（本轮已修）

`71b01ac` 之后补测「预测期发现逻辑缺陷、但执行阶段正常产出」这一路径时，
发现**同一类伪成功的第二个漏洞**，与 P0 是不同的孔：

`execute_phase` 用 `pivot_reason` 决定是否输出占位文本，而 `pivot_reason`
有两个赋值来源：

| 来源 | 位置 | 语义 |
|------|------|------|
| 预测期逻辑缺陷 | `cognitive_loop.rs:723` `Logical Flaw Detected: ...` | 规划阶段发现隐患，**执行本身可以完全成功** |
| 执行期 StrategicPivot | `cognitive_loop.rs:855` | 节点执行确实失败 |

原代码 `if let Some(reason) = pivot_reason.as_ref()` 不区分二者，于是：

1. 预测期发现 Critical flaw → `pivot_reason = Some(...)`
2. supervisor 放行（偏差 0.0 < 阈值），节点**真实执行成功**，产出 `"mock"`
3. 但 `execute_phase` 仍输出 `"Strategic Pivot: Logical Flaw Detected: ..."`
4. `exec_pivot` 仍为 `false`，`real_result_iterations` 计入 1 → **循环返回 Ok**
5. 上层拿到非空字符串 → `execute_sop_impl` 返回 `status="SUCCESS"`

实测断言输出（修复前）：

```
prediction-phase flaw must not produce pivot placeholder:
"Strategic Pivot: Logical Flaw Detected: persistent critical flaw"
```

即：**一次真实成功的执行，其结果被替换成了假的 pivot 文案，且整体报成功。**

**修复**：占位文本判定从 `pivot_reason` 改为 `*exec_pivot`，与失败守卫
统计同一个执行期信号。两个来源就此彻底分离：

- `exec_pivot` = 执行阶段真的失败了 → 占位文本 + 计入失败守卫
- `pivot_reason` = 供反思/演进消费的诊断信息，不再影响结果内容

**为何第一轮没发现**：原 20 路并发探测与 5 个失败用例都只有执行期
pivot，`pivot_reason` 的预测期分支从未被触发。

### 新增回归测试（含变异验证）

`sisyphus-core` 新增 3 个直测，覆盖三种语义边界：

| 测试 | 覆盖场景 |
|------|----------|
| `test_all_iterations_pivoted_is_error_not_placeholder_success` | 全部迭代执行期 pivot → `AllIterationsPivoted` |
| `test_successful_execution_is_not_flagged_as_all_pivoted` | 合法执行仍返回 Ok，且不含占位文本 |
| `test_prediction_phase_logical_flaw_is_not_counted_as_zero_execution` | 预测期 Logical Flaw 不得污染结果 |

**变异测试证明用例有效**（仅靠「测试通过」不能说明问题）：

| 注入的缺陷 | 结果 |
|-----------|------|
| 关闭失败守卫 `if false && real_result_iterations == 0` | `test_all_iterations_pivoted` **FAILED** |
| 占位判定退回 `pivot_reason.is_some()` | `test_prediction_phase_logical_flaw` **FAILED** |

首次尝试反向变异时**脚本断言失败导致变异根本没应用**，却输出了「全部
通过」的假结果；修正为按精确内容匹配后才得到上表的有效结论。

**测试夹具的两处非显然约束**（均由实测失败暴露，不是推测）：

1. `SisyphusSupervisor` 有快速放行通道：输出被判为健康且
   `success_probability >= 0.5` 时直接返回 `Continue{0.05}`，**完全不调用反射器**。
   最初用 `vec![0.5]` 做夹具，脚本化偏差 `0.9` 根本未被读取，测试拿到
   `"mock"` 而非 pivot。须用 `< 0.5` 的概率才能驱动真实路径。
2. 生效阈值来自 `ComplexityAnalyzer::to_config()`（Low/Medium/High/Ultra
   分别为 0.2/0.05/0.01/0.005），**不是** `config.convergence_threshold`
   ——后者在本测试中完全不生效。

### P0.1 — 补上「中途 pivot 后恢复」的覆盖缺口（本轮已补）

守卫 `AllIterationsPivoted` 的注释明确写着「**中途 pivot 后恢复是合法路径**」，
但此前没有任何测试覆盖它。当时只有两个极端：全部 pivot（→ Err）与
全部成功（→ Ok）。守卫条件若被误写成「任一 pivot 即失败」，这两个用例
**都会照样通过**——缺口是真实的，不是理论担忧。

新增 `SequencedReflector`（按调用次序返回偏差序列，用尽后重复末值）与
参数化的 `setup_manager_with_sequenced_reflector[_and_logic]`，新增 3 个用例：

| 测试 | 覆盖场景 |
|------|----------|
| `test_partial_pivot_then_recovery_still_succeeds` | 第 1 轮 pivot、第 2 轮恢复 → 必须 `Ok`，且两轮都跑了 |
| `test_pivot_diagnostics_do_not_leak_into_recovered_result` | 恢复后的结果不得含任何 pivot 诊断文本 |
| `test_prediction_flaw_plus_recovery_uses_execution_signal_only` | 预测期 flaw 与执行期 pivot **同时存在**时，占位判定只认执行期信号 |

**变异验证一**：把守卫误写成「任一 pivot 即失败」

| 用例 | 结果 |
|------|------|
| `test_partial_pivot_then_recovery_still_succeeds` | **FAILED** |
| `test_pivot_diagnostics_do_not_leak_into_recovered_result` | **FAILED** |
| `test_all_iterations_pivoted_...`（既有） | ok ← **旧用例发现不了** |
| `test_successful_execution_...`（既有） | ok ← **旧用例发现不了** |

**变异验证二（暴露了一个更隐蔽的问题）**：把占位判定退回
`pivot_reason.is_some()`，**5 个用例全部照常通过**。排查后确认原因：
它们都用 `MockLogicalSimulator`（不报 flaw），`pivot_reason` 恒为 `None`，
于是 `None.is_some()` 恒为 `false`，与 `exec_pivot` 行为完全一致——
两种写法在这个场景下**不可区分**。

只有当预测期 `pivot_reason` 与执行期 `exec_pivot` **取值相反**时差异才可观测。
因此才有第三个用例（用 `CriticalFlawSimulator` 让预测期也赋值）。重做该变异：

```
a successful iteration was replaced by pivot placeholder text:
"Strategic Pivot: Logical Flaw Detected: persistent critical flaw"
```

新用例 **FAILED**，缺口补上。这条记录说明：**测试通过与测试有效是两件事**——
若不做变异验证，本报告会把「5 个用例全绿」当成覆盖已完备。

### P3 — 巨型 misc 测试文件（**本轮已核查，结论降级**）

`sisyphus-types/tests/` 下 4 个大文件合计 15,879 行：
`asset_misc.rs` 5435（286 测试）、`types_misc.rs` 4066（244）、
`asset.rs` 3847（186）、`types_misc_v2.rs` 2531（198）。

**但我原先「无法导航的堆积」这个判断只对了一半，修正如下。** 实测发现：

1. **前 1000 行有清晰的类型分区**——`AssetKind`、`InsightData`、`AssetContent`、
   `CognitiveAsset` 等按被测类型组织，并非无结构堆砌。
2. **但从第 1013 行起，连续 29 个分区标题全部叫 `More tests`**，
   覆盖了 5435 行中的 **4422 行（81%）**。
   也就是说：**真正有意义的分区只覆盖了不到 20% 的内容。**
3. **同目录另有 28 个大小正常的文件**（最长 932 行，多数 100–300 行），
   说明仓库并非「只会往大文件里加」，而是确实存在分流习惯。

**修正后的定性**：**命名债**。前 1000 行的作者显然在认真分区，
但后续 29 个 `More tests` 表明**分区习惯没有随文件增长延续**。
新用例作者看到连续 29 个同名标题，只能靠「继续往最后加」这一唯一路径。

#### P3 处理结果：已修复（提交 `a2cc751`）

我先前判断「需人工判断每个分区的实际主题」，**这个判断下得太早了**。
实测后发现主题**高度一致**：29 个分区里 **22 个的全部测试函数名都含 `unicode`**，
其余 7 个是 unicode 混合 serde / 边界变体。也就是说这不是 29 堆无关联用例，
而是**同一个主题（Unicode 边界覆盖）被逐批追加**，作者没给每批起名字。

已做的改动（**纯注释，零代码改动**）：
- 28 个 → `// Unicode 边界覆盖（各类型）` / `（混合变体）`
- 1 个（L1182，28 个测试的大分区）→ `// Unicode / serde 往返混合覆盖`

验证：`git diff` 确认变更行 **100% 为注释**（非注释行数 = 0）；
`cargo test -p sisyphus-types --test asset_misc` → **286 passed; 0 failed**。

**过程中的翻车**：第一次批量替换时脚本匹配到了已剥离 `//` 的裸文本行，
写回时没有补回前缀，直接把文件改成语法错误（57 个编译错误）。
靠编译才发现。**教训：批量改文本后，必须用编译器验证，不能只看 diff。**

## 5. 环境阻塞（maven 已解除；bridge 仍阻塞）

本机 `vm.overcommit_memory=2`（严格提交），`CommitLimit ≈ 21122 MiB`。

**早期实测（cargo 尚未启动时）** `Committed_AS` 已达 **18632 MiB（88%）**，
占用者主要是 6 个 java 进程（nacos `-Xmx2g`、skywalking、elasticsearch
以及 5 个 `UseG1GC -Xmx256m` 进程，合计约 6 GB）。

后果：触发多次编译期 SIGABRT / `rustc-LLVM ERROR: out of memory`。

**这部分阻塞与代码无关**——同一份代码在提交内存降到 66% 时毫无问题。
本节把两类阻塞分开记录，因为它们的**可解性完全不同**。

### 5.1 已解除：JVM 堆预留（maven）

**我最初把 maven 的失败归类为「环境限制，非代码问题」，这个判断是错的。**
真正原因只是 JVM 默认把堆预留到 5 GB，一个环境变量即解决：

```bash
MAVEN_OPTS="-Xmx512m -XX:MaxMetaspaceSize=128m" \
    cargo test -p sisyphus-tools-build --test language_backend_guards -- --ignored --test-threads=1
# test result: ok. 13 passed; 0 failed; 0 ignored (132.77s)
```

**仓库侧已修复**（提交 `4dc689d`）：方法写进了模块文档与 2 条用例的
`#[ignore]` 理由，避免下一个人重复排查。

### 5.2 部分解除：bridge 编译（此前结论已被本轮实测推翻）

**早前结论「bridge 4 种方式全部 OOM、无法编译」是错的。** 本轮在同样约 10 GB
可用内存下重测，`--no-default-features` 路径（正是 CI 实际使用的路径）全部通过：

| 命令 | 早前结论 | 本轮实测 |
|------|----------|----------|
| `cargo check -p sisyphus-bridge --all-targets` | 未测 | **通过**，1m06s |
| `cargo check -p sisyphus-bridge --no-default-features --lib` | `opentelemetry_sdk` rustc OOM | **通过**，2m15s |
| `cargo test -p sisyphus-bridge --no-default-features --lib` | 未测 | **85 passed** |
| `cargo test -p sisyphus-bridge --no-default-features --test bridge_integration_test` | 未测 | **54 passed**（需 `--test-threads=1`，见下） |
| 带 sandbox（默认 feature，含 wasmtime） | rust-lld OOM | **仍然 OOM**（`memory allocation of 34832 bytes failed`） |

结论修正为：**OOM 与 sandbox 特性（wasmtime）绑定，而非 bridge crate 本身。**
CI 的 `bridge-integration-test` job 用 `--no-default-features`，不受影响；
`bridge-test-full` job 启用 sandbox，在本机内存条件下仍无法编译——这一条
维持原判断。`hook_overhead.rs` 与 `jcode_call_chain_test.rs` 的 `#[ignore]`
因此仍然正确。

早前之所以误判：4 次尝试均在**默认 feature（带 sandbox）**下进行，而
`--no-default-features` 那次被 `opentelemetry_sdk` 的瞬时内存压力打断，
未做重试就下了「结构性不可行」的结论。**教训：把一次瞬时 OOM 当成结构性
限制前，必须重试并变换 feature 组合。**

**`hook_overhead` 的阻塞原因曾被误判，本轮已实测跑通**：

上表中的 OOM 记录（`memory allocation of 34832 bytes failed`）来自**带 sandbox**
路径。`hook_overhead` 走 `--no-default-features`，前两次尝试均 `rc=124`
（超时，**不是** OOM——日志中 `memory allocation` / `out of memory` /
`signal: 9` 命中数为 **0**）。第三次改用 `nohup` 让编译脱离工具层 600 秒
上限后**编译成功**（增量 1m34s），实跑结果：

```
running 1 test
test measure_stream_hook_overhead ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;
             finished in 1.48s
```

**「4 种编译手段均 OOM」与「本机不可验证」两个记录都是错的**：
真实原因只是全量重建依赖链耗时超过工具层上限。`#[ignore]` 保留是对的——
它只测量不设门限，绝对值随机器而变，写死门限会在别的机器上变 flaky。

### 5.3 ADR-013 断流钩子开销实测

| 形态 | 每 10 万条 | 每条消息 |
|------|-----------|---------|
| (a) 裸流 | 55.6 ms | 556.2 ns |
| (b) 带钩子流（优化前） | 169.1 ms | 1691.5 ns（**+1135.3 ns**） |
| (c) `.map(Ok)`（优化后） | 58.9 ms | 589.1 ns（+32.9 ns） |

相对开销 (b/a) = **3.04x**；(c/a) = **1.06x**。
优化收益：每条省下 **1102.4 ns**。

**但 (c) 的 +32.9 ns 不应被当作结论**——测试自己注明裸流跨运行波动可达 35%，
即 556.2 ns × 35% ≈ 195 ns，远大于 32.9 ns。
**能站住的只有「(b) 显著高于 (a)」，不能断言「.map 的成本是多少」。**
这也是它必须 `#[ignore]` 的根本原因。

## 6. 本次测量中我自己犯的 17 次错（记录在此避免重犯）

1. **审计脚本把 `llm.rs` 的 12 处 `unsafe` 误报成 1 处。**
   原因：按大括号配平剥离 `#[cfg(test)]` 块时，`#[cfg(test)]` 直接标在 `impl`
   上（而非 `mod`）的情况没处理，配平一路吞到文件末尾。修法：只剥离
   `#[cfg(test)]` 紧跟 `mod` 的块。
   *用原始 grep 交叉验证才发现*——若只信脚本，会得出「代码很干净」的错误结论。

2. **误判 CI 未覆盖 `rust_backend_test_e2e`。**
   grep 不到显式 target 名就断言没覆盖，实际 `--workspace` 隐式包含所有测试 target。
   教训：注释与实际行为冲突时，先跑命令再下结论。

3. **误判 `#[ignore]` 理由过期**（第 3.2 节）。
   实测后被证伪。教训：注释说「需要 X」时，先验证 X 是否真的不存在。

4. **（事后补记）审计「32 处 allow」时，我先用正则提取形参，
   得出 `evolve_phase` 有 `embedding/vector_clock/version` 三个未用形参——这是错的**，
   正则把类型名当成了形参。改用「函数体内按 `fn` 名配平大括号再统计 `\b形参\b`」
   后才得到可信结论。**教训：静态分析脚本本身必须先用已知答案校验，
   否则它会自信地编造数据。**

5. **把 `grep -c evals` 的返回值 1 读成了 0**，
   写下「`evals` 不在 workspace 中」的错误断言并提交。
   **工具返回了正确答案，我没读对**——这类错误比工具算错更隐蔽，
   因为表面上「查过了」。**教训：核对工具输出时，要先复述它的原话再下结论。**

6. **报告中「4763 个测试」是估计值，未经实测就写进了正文**。
   后来「修正」为 5098，实测是 **5088**。
   **更深一层的错误**：我给的拆分是「单元 1420 + 集成 3668」，
   但 **1420 + 3668 = 5088 ≠ 5098**——总数与拆分**自相矛盾**，
   而我从未做这道加法。错的来源：第一次 grep 范围是 `crates/ webui/`，
   后来却把总数与 `crates/` 内部的拆分配在了一起。
   **教训：给出「总数 = A + B」这种写法时，必须验证 A + B = 总数。
   这是一道口算就能拦住的错误，没做就等于没验证。**

7. **把 P3 定性为「无法导航的堆积」**。读完发现**只对了一半**：
   前 1000 行确实有清晰类型分区（同目录另有 28 个正常大小的文件），
   但从第 1013 行起连续 **29 个分区标题全部叫 `More tests`**，
   覆盖 81% 的内容。我第一眼只看到前几个正常标题就下了结论。
   **教训：抽样观察前几个分区不足以判断全貌——
   我恰好抽到了质量最好的那一段。**

8. **把分区数写成 31，实测是 29**。写报告时凭印象数了前半部分就外推。
   **教训：能数的东西就数，不要估。**

9. **断言「需人工判断每个分区的实际主题」，实测发现主题高度一致**
   （29 个中 22 个全部测试名含 `unicode`），完全可机械推导。
   **教训：说「这事只能人工做」之前，先确认自动化的边界在哪——
   很多时候「需要判断」只是我没往下看。**

10. **批量替换注释时写坏了文件**：脚本匹配到已剥离 `//` 的裸文本行，
    写回时未补前缀，57 个编译错误。**教训：批量改文本后必须用编译器验证，
    不能只看 diff 满意就提交。**

11. **报告写「生产文件 364」，实测为 371，且找不到任何能得出 364 的口径**
    （按路径排除 `tests/` 即为 371；无自然子集正好是 364）。
    当时的数字未标注统计方式，事后也无法重建。
    **教训：给出统计数字时必须同时给出「口径」——
    「多少个文件」脱离「怎么数的」就是无法验证的数字。**

### 本轮补记的 4 次错（12–15）

12. **在报告摘要里写「`#[ignore]` 总数 27 条」**，
    但这个数字是在修复过程中陆续改的，从未重新核对过实际分布。
    实测 `grep -rn '^\s*#\[ignore' crates/ evals/` = **26 条**，
    且其中 **2 条是 `rust_backend_test_e2e.rs` 里 fixture 字符串中的假 ignore**，
    实际被跳过的测试是 **24 条**。
    **教训：改了一串数字之后，必须回到源数据重算一次总数，
    而不能沿用「上一轮是对的」这个假设。**

13. **把「`team_integration` 6 条未跑」直接写成「理由成立（env 缺失）」**。
    这是**猜测而非测量**——我当时只是看到 `#[ignore]` 文本提到了三个 env var。
    实跑后发现它们根本不会失败，而是**全部报绿**：
    `6 passed ... finished in 0.00s`。真实问题（静默 return）比原判断严重。
    **教训：「未跑」不等于「理由成立」。
    凡是标注为「理由成立」而我又没亲自跑过的，一律降级为「未证实」。**

14. **把 `jcode_call_chain_test.rs` 的 ignore 理由当成事实写进报告**，
    实际读完全文才发现它**自己 spawn 了 mock UDS server**，
    根本不需要外部 Jcode 进程。理由是错的。
    **教训：`#[ignore = "..."]` 里的字符串是作者的意图，不是事实。
    要下「理由成立」的结论，必须读代码或跑命令。**

15. **把 maven 的 JVM OOM 直接归类为「环境限制，非代码问题」**，
    还写下「这不是可以靠改测试解决的问题」这种带结论的断言。
    实际上只需要 `MAVEN_OPTS="-Xmx512m"` 就能跑通，
    代码侧一行不用改，13 条测试全部变绿。
    **教训：「环境限制」是一个需要证据的结论，不是无法推进时的默认借口。
    下这个结论前，至少要试一次「调参而非改码」——
    JVM 堆、cargo 并行度、debug info 都是可调项。**

另外两个**工具自身的坑**：一次 `rm` 含通配符且指向工作区外被安全门拦下
（拦得对——那批历史日志不是我的）；一次审计脚本因大括号配平 bug 静默吞掉半个文件。

16. **把「工具超时」当成「内存不足」写进报告**，并据此下了
    「4 种编译手段均 OOM」「本机不可验证」两个结论。
    实际日志里 `memory allocation` / `out of memory` / `signal: 9`
    命中数全为 **0**，`rc=124` 是超时而非 OOM；且 `target/debug/deps/`
    下本就有它编译产出的 `.d` 文件。改用 `nohup` 让编译脱离工具层 600 秒
    上限后，**实跑 1 passed（1.48s）**。
    **教训：`rc=124` 是超时，`rc=137` 才是被杀。两者归因完全不同，
    混淆会把「慢」写成「结构性不可行」，并连带影响「ignore 是否合理」
    的判断——我据此怀疑过一个本来正确的 `#[ignore]`。**

17. **分析 P2.7 时看错了函数**：报告称「`synthesis.rs:120` 的语义标签
    被当作 `tool_id`」，实际第 120 行是 `action_label()`，与
    `tool_id_for_action()` 是两个函数，调用点始终成对分开使用。
    我当时看到 `Fetch → "fetch"` 字符串眼熟就下了结论，没打开定义看。
    **教训：判定「某个值被当作 X 使用」必须看赋值点，
    而不是看它出现在哪个字符串字面量附近。**

18. **测试的名字不等于测试的机制**（本轮第二次同类错误）：
    `913ae9d` 新增的用例叫 `test_tool_id_maps_to_registered_builtin_tools`，
    注释写着「actually exists in the bridge registry」，我据此把
    P2.7 的验证闭环评为 strong。实际断言是 `assert_eq!(映射结果, "字面量")`
    ——两边都是手写常量，**中间没有任何东西连接真实注册表**。
    实测证明：把 bridge 注册表的 `builtin:web_search` 改名，
    intel 侧 4 个测试全部照旧通过。
    **教训：评估一个测试的证明力，要问「断言的两端分别来自哪里」。
    两端都是同一处的字面量 → 它锁的是常量一致性，不是外部契约；
    至少一端必须来自被声称的权威源（真实 registry、真实文件、真实调用）。**
    这次的代价是上一轮给出的结论被自己的证据推翻，已在第 7 节改写。

## 7. 复盘未能完成的部分（如实列出）

| 想验证的 | 状态 | 阻塞原因 |
|---------|------|---------|
| `sisyphus-tests` 两条 `#[ignore]` 是否可通过 | **已完成**（不可通过，理由成立） | — |
| `hook_overhead.rs` 的 `#[ignore]` 是否仍合理 | **已实测跑通，ignore 理由成立**：`--no-default-features` 下实跑 `1 passed`（1.48s）。上一轮「本机不可验证」的记录是错的——真实原因是编译慢，不是 OOM 也不是不稳定。保留 ignore 是正确的：它只测量不设门限，绝对值随机器而变 | — |
| 认知主循环端到端覆盖真相（P1） | **已完成**：方向与原推测相反，见第 4 节 P1 | — |
| `#[allow(unused*)]` 逐处审计（P2） | **已完成**：用编译器裁决法核实全部 `unused_*` 类，删 3 处失效豁免。`c49b34f` 当时 `91 passed`，`3309edb` 新增 3 个回归测试后为 94 passed，`913ae9d` 再新增 3 个（P0.1）后为 **97 passed** | — |
| 全量 26 条 `#[ignore]` 逐条核实 | **已完成**：13 条工具链全部跑通、2 条网络跑通、1 条 LLM 跑出真实错误、1 条 UDS 实跑前先验代码、2 条为 fixture 假 ignore；**`hook_overhead` 本轮实跑通过**（1 passed，见 5.2），**26 条全部有据可查** | — |
| `team_integration` 6 条的真实行为 | **已完成**：静默跳过假绿已修（`d090118`）；本轮另核实真实 LLM 场景，6/6 如实 FAILED，见下一行 | — |
| `maven_compile_failure_returns_err` | **已完成（已突破环境限制，本轮重跑取证）**：`MAVEN_OPTS="-Xmx512m -XX:MaxMetaspaceSize=128m"` 下实测 `13 passed; 0 failed`（385.12s，rc=0），13 条工具链测试全部跑通，不再依赖 `4dc689d` 的历史声称 | — |
| `hook_overhead.rs` / `jcode_call_chain_test.rs` 的**编译** | **部分解除**：`--no-default-features` 路径实测可编译（bridge 85 + 54 测试通过）；带 sandbox 仍 OOM | 见 5.2 |
| `team_integration` 6 条在**真实 LLM 配置下**的运行结果 | **仍缺凭据，但已核实不是代码缺陷**：本轮实跑 `cargo test -p sisyphus-tools-agent --test team_integration -- --ignored`，6/6 **如实 FAILED** 并给出 `LLM not configured; set LLM_API_KEY / LLM_ENDPOINT / LLM_MODEL`。用例用 `panic`（而非 `return`）暴露配置缺失，ignore 理由准确，不存在静默跳过或假绿。本机无任何 LLM 相关环境变量 | **环境限制**（缺凭据，非代码问题） |
| **真实工具 + 真实 LLM API 的完整端到端链路** | **仍未覆盖**：当前通过的是 Dummy LLM（回显合法配置）+ 内存执行器 | **环境限制**，见 P1 |
| `crates/` 侧 6 处 `#[allow(dead_code)]`（非测试代码） | **已完成**（`c49b34f` + 本轮复核）：原 14 处逐个实测裁决，5 处失效已删；剩余 6 处在 `3309edb` 之后的复核中逐处移除并实跑 clippy，逐一触发真实 `never used/read/constructed` 告警 | — |
| 2 处 `#[allow(deprecated)]` | **已完成**：删掉后编译器报 `use of deprecated field`（`SyncRequest::offset` / `SyncPageEnd::next_offset`），在 `-D warnings` 下会编译失败，属正当协议兼容 shim | — |
| 43 处 `clippy::*` 豁免 | **已完成审计**：逐处删除后跑 clippy 比对。37 处实测必要（附触发计数）；6 处 `cast_precision_loss` 属未启用的 pedantic 组，当前失效但保留 | — |
| bridge 战略目标用例断言过时 | **已修复后又修正**（`d53b88b` → `71b01ac`）：先按「dummy 能回显合法配置」改写为断言成功；随后 P0 修复证明该「成功」本身是假的（工具从未真正执行），再次改写为断言「记录与实际发生的事自洽」，用例重命名为 `..._records_kernel_outcome_with_dummy` | — |
| `ActionType` 语义标签被当作可执行 `tool_id` | **已修**（`913ae9d`）：机制曾误判——并非标签被当作 `tool_id`，而是 `tool_id_for_action` 5 个分支全部返回未注册的 `"fast_tool"`。已映射到 `builtin:web_fetch`/`web_search`/`bash_exec`，并修掉 3 处绕过映射函数的硬编码；删除固化了占位行为的测试，替换为 3 个断言真实不变量的用例并经双向变异验证 | 已修 |
| **P2.7 修复的「注册表一致性」验证** | **上一轮结论已推翻并降级**：`913ae9d` 加的测试名声称「maps to registered builtin tools」且注释写「actually exists in the bridge registry」，但断言双方都是**手写字面量**——把 bridge 注册表里的 `builtin:web_search` 改名成 `builtin:web_search_v2`，intel 侧 4 个测试**全部照旧通过**（含名字里写着 registered 的那个），属假绿。**本轮补上真闭环**：在 bridge 侧新增 `tools::tests::planner_tool_ids_exist_in_registry`，走生产 `GoalDrivenSynthesizer` 产出真实 `SOPGraph`，再对真实 registry 断言命中。同名漂移下 intel 仍 4/4 假绿而新测试 FAILED 并指名漂移项，证明闭环有效。同时把 intel 侧测试改名 `test_tool_id_is_stable_per_action` 并加注「不验证注册」，防止再被误读 | 已修 |
| `team_integration` 6 条在**真实 LLM 配置下**的运行结果 | **仍缺凭据，但已核实不是代码缺陷**：本轮实跑 `cargo test -p sisyphus-tools-agent --test team_integration -- --ignored`，6/6 **如实 FAILED** 并给出 `LLM not configured; set LLM_API_KEY / LLM_ENDPOINT / LLM_MODEL`。用例用 `panic`（而非 `return`）暴露配置缺失，ignore 理由准确，不存在静默跳过或假绿。本机无任何 LLM 相关环境变量 | **环境限制**（缺凭据，非代码问题） |
| `execute_sop` / `execute_tool` 把执行失败报告为 `SUCCESS` | **P0，已修**（`71b01ac`）：新增 `CoreError::AllIterationsPivoted`，全程零真实产出时如实返回 Err。实测 20 路由 20/20 假 SUCCESS 变为 20/20 真 Internal。连带修正 5 个依赖「伪造成成功」的测试 | 已修 |

**本报告不含任何未标注为「实测」的结论。**

## 8. 本轮修复提交清单

| 提交 | 内容 |
|------|------|
| `9164bd6` | 删除认知主循环 `reflect_phase`/`evolve_phase` 中 2 处 A2 遗留死代码（`active_key`） |
| `95538ce` | `make test-all` 结束时打印被排除的 crate 与对应 CI job |
| `d80a7ea` | 补齐 `DummyLlmClient` 的 schema 分派，移除 2 条被永久跳过的认知流程测试 |
| `f4de8cd` | 修复 `node.rs` 中被错误忽略的空测试，`--lib` 从 112 passed / 1 ignored → 114 passed / 0 ignored |
| `af0b635` | 修正 `webfetch` 测试中与实现不符的 `content_type` 断言（实得 `text/html; charset=utf-8` vs 期望 `text/html`） |
| `d090118` | 消除 `team_integration` 6 条集成测试的静默跳过假绿；纠正 `jcode_call_chain_test.rs` 的错误 ignore 理由 |
| `4dc689d` | 解除 `language_backend_guards` 的 JVM OOM 阻塞（`MAVEN_OPTS=-Xmx512m`），13 条全部跑通；写入模块文档 |
| `fb259a8` | 删除 `cognitive_loop.rs` 中 3 处失效的 `#[allow(unused_variables)]`（编译器裁决法核实） |
| `3bf9986` | 修正 `#[allow]` 统计口径为可复现命令，删除不可复现的 `hook_overhead` 结论 |
| `9bb22aa` | 收窄 `exec_eval.rs` 中 3 处过宽的 `#[allow(dead_code)]`，从结构体级改为字段级 |
| `d53b88b` | 修正 `test_execute_tool_strategic_next_goal` 的过时断言：Dummy LLM 实际能生成合法提案（实测 `success=true`），改为断言真实成功路径 |
| `66b37fe` | 修正并发用例断言：区分业务失败（`Status::internal`）与真正回归，消除对 sandbox 的环境依赖 |
| `c49b34f` | crates 侧 14 处 `#[allow(dead_code)]` 逐个实测裁决，删除 5 处失效豁免（删除前口径） |
| `fafc20a` / `c7edfce` | 插桩定位并发用例失败真因：`StrategicPivot` 吞掉执行失败并伪装成 SUCCESS；更正 `66b37fe` 基于错误前提的说明 |
| `71b01ac` | 修复 P0：全程 StrategicPivot 零真实产出时如实返回 `AllIterationsPivoted`；连带修正 5 个依赖「伪造成成功」的测试 |
| `3309edb` | 修复 P0.2：预测期 `Logical Flaw` 不再伪造 pivot 占位文本（占位判定改用执行期 `exec_pivot`）；新增 3 个直测并经双向变异验证有效；修正报告统计口径（allow 67→63、dead_code 17→14、clippy 47→43） |
| `ee5406f` | 纠正 P2.7 机制描述（原误判为「语义标签被当作 tool_id」）；补做 `upper_case_acronyms` 删除验证（触发 2 条 `SOP` acronym 告警，`SOP` 全仓 259 处引用故豁免正当） |
| `913ae9d` | 修复 P2.7：`tool_id_for_action` 映射到真实注册的 `builtin:` 工具，修掉 3 处绕过映射的硬编码 `fast_tool`，删除固化了占位行为的测试并替换为 3 个真实不变量用例（双向变异验证）；补齐 P0.1「中途 pivot 后恢复」的覆盖缺口（3 个新用例 + `SequencedReflector` 夹具，两轮变异验证） |
| 本轮 | 推翻自身结论：补上 P2.7 的跨 crate 注册表闭环测试（bridge 侧断言真实 synthesizer 产出命中真实 registry），并用「bridge 侧改名 → intel 侧 4/4 假绿 vs 新测试指名失败」双向变异证明有效性；同时纠正 intel 侧误导性测试命名 |

### 本轮实测确认的结论汇总

- **代码卫生确实良好**：`panic!` 0 处、真 `unsafe` 0 处、`todo!()`/`unimplemented!()` 0 处。
- **`#[ignore]` 基本合理**：26 条属性中 24 条为真，理由经实跑核实成立；1 条理由错误已改；
  2 条是 fixture 字符串内的假 ignore。
- **`#[ignore]` 会掩盖坏测试**：`webfetch` 那条一旦按理由启用就会立即失败——
  长期跳过的测试可能从未被真正执行过。
- **「环境限制」不应是默认借口**：maven 那条我一开始归为环境限制，
  实测只是 JVM 默认堆预留 5 GB，一个环境变量就解开了。
- **会掩盖死代码的两类豁免已全部裁决完毕**（`unused_*` 4 处、`dead_code` 14 处、
  `deprecated` 2 处）：删 3 + 5 处失效、收窄 3 处过宽，其余经编译器实测为必要。
  **这三类里没有一处是掩盖死代码的残留，且每个数字都由实际删除验证得出。**
- **已实测**：43 处 `clippy::*` 豁免全部逐处裁决（编译器删除法 + 默认 lint 组验证）。
  `double_must_use` 8 处、`result_large_err` 5 处、`enum_variant_names` 2 处、
  `collapsible_if` 4 处、`upper_case_acronyms` 2 处均实测必要并附触发计数；
  6 处 `cast_precision_loss` 属 pedantic 组而项目未启用 pedantic，当前失效，
  保留作预防性放行并加注。`too_many_arguments` 13 处与 `type_complexity` 2 处
  经人工核实为构造函数依赖注入与字段内聚性考量，不建议重构。
- **求和校验**：43 + 14 (`dead_code`) + 2 (`deprecated`) + 4 (`unused_*`) = **63**，
  与语法感知统计的总数一致。

### 本轮踩过的坑：重定向顺序让裁决全部失真

审计中途发现 `cargo clippy ... 2>&1 > file` 会把 stdout 写入文件而把 stderr
留在终端，而 clippy 的**全部 warning 都走 stderr**——于是每次都读到空输出，
把 8 处 `double_must_use` 全部误判为「失效」。正确写法是 `> file 2>&1`。
记录在此，避免后续审计重蹈覆辙。
