# 已知阻塞：合成 SOP 缺少工具参数，`execute_goal` 必然失败

**状态**：未修复（需产品决策）
**影响面**：CI `test-integration` job 红灯；生产 API `POST /api/execute/goal` 不可用
**首次定位**：2026-10-06

> **修复所需规模超出单次缺陷修复**：需给公开类型 `WasmSOPNode` /
> `NodeKind::Wasm` / `SOPNodeConfig` 增加参数字段，牵涉 71 处构造点，
> 属公开 API 变更，须先决定参数来源（LLM 合成 / 用户输入 / 上下文推导）。
> 本文只做定位与方案记录，未实施任何代码改动。

## 现象

`cargo test -p sisyphus-tests --lib` 中 2 个用例失败：

```
test tests::test_full_pipeline ... FAILED
test tests::test_concurrent_execution ... FAILED
Goal execution failed: AllIterationsPivoted(1,
  "Strategic Pivot: Node fetch produced output with deviation score 0.81
   (effective threshold 0.01, base 0.01)")
```

## 根因链路（已逐环核实）

1. `GoalDrivenSynthesizer::synthesize`（`sisyphus-intel/src/synthesis.rs:190`）
   为动词 `fetch` 造出节点，`tool_id_for_action` 映射到 `builtin:web_fetch`
   （`synthesis.rs:125`）。

2. `WasmSOPNode::execute`（`sisyphus-types/src/node.rs:42`）**硬编码**：
   ```rust
   let args = Vec::new();
   ```
   无论 `tool_id` 是什么，都以空参数调用 executor。

3. `cmd_web_fetch`（`sisyphus-tools-executor/src/executor/process_cmds.rs:75`）
   要求 `url` 必填：
   ```rust
   let url = str_arg(&v, "url")?;   // → "missing or non-string arg 'url'"
   ```

4. 节点失败 → `StandardReflector` 按 `failure_rate * 0.6 + severity * 0.3`
   算出偏差 0.81 → 越过 High 档 0.01 阈值 → `AllIterationsPivoted`。

**不是网络问题**：失败发生在发起请求之前。

## 为什么同文件的 3 个用例能过

| 用例 | 路径 | executor | 结果 |
|------|------|----------|------|
| `test_dag_execution` | 手写图 + `tool_id: "hello"` | `DummyToolExecutor`（任意 id 返回 `ok`） | 通过 |
| `test_cognitive_loop` | 手写图 | 同上 | 通过 |
| `test_wasm_tool_chain` | 手写图 | 同上 | 通过 |
| `test_full_pipeline` | `execute_goal` → 合成 | 真实 `ExecutorRouter` → `builtin:web_fetch` | **失败** |
| `test_concurrent_execution` | `execute_goal` → 合成 | 同上 | **失败** |

同文件内测试夹具分层不一致：手写图配假 executor 因此不受参数缺失影响；
走真实合成路径的则必然撞上。

## 影响范围不止测试

`execute_goal` 是 bridge 的生产 API：

- `crates/sisyphus-bridge/src/http/handlers.rs:242`（`POST /api/execute/goal`）
- `crates/sisyphus-bridge/src/http/mod.rs:178`（路由注册）

调用链已逐环核实：

```
execute_goal                       // kernel/execution.rs:208
  └─ prepare_goal_execution        // kernel/execution.rs:36
       └─ intelligence_manager.synthesize_goal   // intelligence_manager.rs:50
            └─ self.synthesizer.synthesize(...)
                 └─ GoalDrivenSynthesizer       // bridge_service.rs:505（生产装配）
```

即：**任何经该 API 提交的目标，当前都无法执行成功**。这不是测试夹具问题。

## 三个 builtin 命令全部要求必填参数

| 命令 | 必填参数 | 位置 |
|------|----------|------|
| `bash_exec` | `command` | `process_cmds.rs:22` |
| `web_search` | `query` | `process_cmds.rs:49` |
| `web_fetch` | `url` | `process_cmds.rs:77` |

因此**任何**经 `GoalDrivenSynthesizer` 合成的 SOP 都无法执行，与目标措辞无关。

## 修复方案（需决策后实施）

参数在类型系统中完全缺失，需贯穿两层：

```
SOPNodeConfig { name, kind }          // graph.rs:103  —— 无参数
  └─ NodeKind::Wasm { tool_id, fuel_limit }   // node.rs:69 —— 无参数
       └─ WasmSOPNode::execute → args = Vec::new()   // node.rs:42 —— 硬编码空
```

### 方案 A：给节点增加参数字段（推荐）

1. `WasmSOPNode` 增加 `args: Vec<u8>`（或 `serde_json::Value`）；
2. `NodeKind::Wasm`、`SOPNodeConfig` 同步增加 `args`；
3. `execute` 用 `self.args` 而非 `Vec::new()`；
4. `GoalDrivenSynthesizer` 按动作生成默认参数。

**代价**：`WasmSOPNode` 有 71 处构造点，需全部补字段（或加 `Default` + `..Default::default()`）。
属于公开类型变更，需评估下游兼容。

### 方案 B：执行期从上下文取参

节点参数不固化在图里，执行时从 `ExecutionContext` 的 notepad / 上游输出读取。

**代价**：隐式数据流，调试困难；需定义明确的参数命名约定。

### 方案 C：合成阶段就用真实参数填充

`GoalDrivenSynthesizer` 直接为每个动作生成完整 args（如 `fetch` 填默认 URL）。

**代价**：治标；仍需方案 A 的类型支持才能落地。

## 不建议的做法

- **不要**把这 2 个用例改成 `#[ignore]` 或放宽断言——会掩盖生产 API 不可用的事实；
- **不要**在 `str_arg` 上加默认值——会让工具在缺参时执行出错误结果，比失败更危险。

## 已排除的假设

- ❌ 网络不可达 / 反爬——失败发生在请求发起前；
- ❌ `DummyLlmClient` 恒返 0.1——该路径用的是 `StandardReflector`，与桩无关；
- ❌ supervisor 阈值塌缩——已在 41e6033 修复，但单独修复不足以让用例转绿。
