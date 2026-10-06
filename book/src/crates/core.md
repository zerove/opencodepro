# sisyphus-core — 内核与编排

核心 crate，编排整个认知循环，依赖所有其他 crate。

## 关键类型

### `SisyphusKernel`

认知循环的主要入口，持有所有子系统的引用：

```rust
pub struct SisyphusKernel {
    pub state_store: Arc<dyn StateStore>,
    pub tool_executor: Arc<dyn ToolExecutor>,
    pub intel: Arc<SisyphusIntel>,
    pub synthesizer: Arc<dyn FlowSynthesizer>,
}
```

#### 核心方法

| 方法 | 说明 |
|--------|-------------|
| `execute_goal(goal, sop_id)` | 完整流水线：合成 → 执行 → 认知 → 演进 |
| `execute_goals(goals)` | 并发执行多个目标，返回组合输出（不转发事件） |
| `execute_goals_with_events(goals, event_tx)` | 同上，但把各子图执行事件转发到 `event_tx`，供 SSE 订阅 |
| `execute_with_cognition(graph, start, sop_id, ctx)` | 多迭代循环：预测 → 执行 → 反思 → 演进 |
| `load_sop(sop_id)` | 从状态存储加载已保存的 SOP 图 |
| `save_sop(sop_id, graph)` | 将 SOP 图持久化到状态存储 |

### `Executor`

按拓扑序并行执行 SOP 图的 DAG 运行器：

```rust
let ctx = Executor::new()
    .execute_with_context(&sop_graph, start_node, None)
    .await?;
```

### `CognitiveReport`

从 `execute_with_cognition` 返回：

```rust
pub struct CognitiveReport {
    pub deviation_score: f64,
    pub iterations: usize,
    pub trace: String,
    pub improvements: Vec<String>,
}
```

### `DependencyResolver`

解析组合图之间的依赖映射：

```rust
pub fn resolve(composite: &CompositeGraph) -> Result<Vec<String>, String>;
```

## 执行流程

1. **合成**：`FlowSynthesizer::synthesize()` 将自然语言目标转换为 `SOPGraph`
2. **执行**：`Executor::execute_with_context()` 按拓扑序运行 DAG，独立节点通过 `tokio::task::JoinSet` 并行执行
3. **模拟**：`MonteCarloSimulator` 通过随机路径采样估算成功概率与成本
4. **反思**：`StandardReflector` 对比预测与实际轨迹，产生偏差分数
5. **演进**：偏差 > 阈值时 `StandardEvolver` 变异 SOP（燃料调整、重试包装）
6. **循环**：重复直到收敛或达到最大迭代次数

## 基准二进制

`cargo run --release -p sisyphus-core --bin integration_stress` 运行端到端进程内压力测试。

## 示例

```rust
use sisyphus_core::{SisyphusKernel, Executor, SOPGraph, WasmSOPNode};
use sisyphus_sandbox::WasmToolExecutor;
use std::sync::Arc;

// 构建单节点 SOP
let executor = Arc::new(WasmToolExecutor::new()?);
let tool = WasmSOPNode {
    name: "greet".into(),
    tool_id: "hello".into(),
    executor: executor.clone(),
    fuel_limit: 1000,
};
let mut graph = SOPGraph::new();
let n = graph.add_node(Arc::new(tool));
let ctx = Executor::new().execute_with_context(&graph, n, None).await?;
```