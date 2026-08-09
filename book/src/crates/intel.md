# sisyphus-intel — 认知与模拟

Sisyphus 的"大脑" — 四项核心认知能力：**流程合成**（flow synthesis）、**模拟**（simulation）、**反思**（reflection）与**演进**（evolution）。

## 四个认知 traits

### 1. FlowSynthesizer（合成）

将自然语言目标转换为类型化动作的 DAG：

```rust
#[async_trait]
pub trait FlowSynthesizer: Send + Sync {
    async fn synthesize(&self, goal: &str, context: &ExecutionContext)
        -> Result<SynthesisResult, String>;
}
```

**实现：`GoalDrivenSynthesizer`** — 基于关键词的解析器，将动作动词（fetch、process、store、search、validate）映射为线性或并行 DAG 流水线。

### 2. Simulator（预测）

运行前估算成功概率与执行成本：

```rust
#[async_trait]
pub trait Simulator: Send + Sync {
    async fn simulate(&self, graph: &SOPGraph, ctx: &ExecutionContext, sop_id: &str)
        -> Result<SimulationResult, String>;
}
```

**实现：`MonteCarloSimulator`** — 对 DAG 进行随机路径采样。计算混合路径概率与估算成本。使用状态存储中的执行历史，使采样偏向历史上成功的路径。

### 3. Reflector（分析）

对比预测与实际结果：

```rust
#[async_trait]
pub trait Reflector: Send + Sync {
    async fn reflect(&self, trace: &str, prediction: &SimulationResult)
        -> Result<ReflectionReport, String>;
}
```

**实现：`StandardReflector`** — 基于字符串的错误分类，构建偏差分数（0.0 = 完全匹配），生成结构化改进建议。

### 4. SOPEvolver（自适应）

反思后变异 SOP：

```rust
#[async_trait]
pub trait SOPEvolver: Send + Sync {
    fn evolve(&self, graph: &SOPGraph, report: &ReflectionReport, prediction: &SimulationResult)
        -> Result<SOPGraph, String>;
}
```

**实现：`StandardEvolver`** — 用重试逻辑包装失败节点，调整燃料预算。使用 `SimpleFuelPricer`（线性）或 `AdaptiveFuelPricer`（指数退避）。

## 错误分类

`ErrorCategory` 对执行失败进行分类：

```rust
pub enum ErrorCategory {
    ResourceExhaustion(&'static str),
    Timeout,
    DataError,
    Unknown,
}
```

`StandardReflector` 使用它判断严重级别并生成有针对性的改进建议。

## 示例

```rust
use sisyphus_intel::*;

let executor = Arc::new(MockExecutor);
let synthesizer = GoalDrivenSynthesizer::new(executor);
let ctx = ExecutionContext::new();
let result = synthesizer.synthesize("fetch process store", &ctx).await?;
println!("Graph has {} nodes", result.graph.graph.node_count());
```