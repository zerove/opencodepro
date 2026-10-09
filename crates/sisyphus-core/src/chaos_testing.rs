//! Sisyphus 的混沌工程框架。
//!
//! 提供的工具：
//! - 故障注入（超时、崩溃、网络故障）
//! - DAG 边界条件测试
//! - 存储并发压力测试
//! - 代理团队韧性验证

use crate::CoreError;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

// ---------------------------------------------------------------------------
// 故障注入
// ---------------------------------------------------------------------------

/// 可注入系统的故障类型。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FaultType {
    /// 模拟 LLM 超时（延迟 + 错误）。
    LlmTimeout,
    /// 模拟 LLM 返回垃圾/无法解析的响应。
    LlmGarbage,
    /// 模拟工具执行崩溃。
    ToolCrash,
    /// 模拟工具执行挂起（永不返回）。
    ToolHang,
    /// 模拟存储写入失败。
    StorageWriteFailure,
    /// 模拟存储读取失败。
    StorageReadFailure,
    /// 模拟组件之间的网络分区。
    NetworkPartition,
    /// 随机延迟注入（最多 max_ms）。
    RandomDelay(u64),
}

/// 混沌实验的配置。
#[derive(Debug, Clone)]
pub struct ChaosExperiment {
    pub name: String,
    pub fault_types: Vec<FaultType>,
    /// 每次操作注入故障的概率（0.0 - 1.0）。
    pub fault_probability: f64,
    /// 实验自动停止前的运行时长。
    pub duration: Duration,
    /// 是否对所有操作注入故障，还是仅针对目标子集。
    pub target_operations: Option<Vec<String>>,
}

/// 混沌实验的结果。
#[derive(Debug, Clone)]
pub struct ChaosExperimentResult {
    pub name: String,
    pub duration: Duration,
    pub operations_attempted: u64,
    pub operations_succeeded: u64,
    pub operations_failed: u64,
    pub faults_injected: u64,
    pub system_recovered: bool,
    pub observations: Vec<String>,
}

impl ChaosExperimentResult {
    /// Success rate.
    pub fn success_rate(&self) -> f64 {
        if self.operations_attempted == 0 {
            return 1.0;
        }
        self.operations_succeeded as f64 / self.operations_attempted as f64
    }
}

/// 混沌实验运行器。
pub struct ChaosRunner {
    /// 混沌注入是否处于激活状态。
    enabled: Arc<AtomicBool>,
    /// 已注入故障的计数器。
    fault_count: Arc<AtomicU64>,
    /// 总操作数计数器。
    op_count: Arc<AtomicU64>,
    /// 当前实验配置。
    current_experiment: parking_lot::Mutex<Option<ChaosExperiment>>,
}

impl Default for ChaosRunner {
    fn default() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            fault_count: Arc::new(AtomicU64::new(0)),
            op_count: Arc::new(AtomicU64::new(0)),
            current_experiment: parking_lot::Mutex::new(None),
        }
    }
}

impl ChaosRunner {
    /// New.
    pub fn new() -> Self {
        Self::default()
    }

    /// 启动混沌实验。
    pub fn start_experiment(&self, experiment: ChaosExperiment) {
        self.enabled.store(true, Ordering::SeqCst);
        self.fault_count.store(0, Ordering::SeqCst);
        self.op_count.store(0, Ordering::SeqCst);
        *self.current_experiment.lock() = Some(experiment.clone());

        info!(
            name = %experiment.name,
            faults = ?experiment.fault_types,
            prob = experiment.fault_probability,
            "Chaos experiment started"
        );
    }

    /// 当前有混沌实验在运行时返回 `true`。
    pub fn is_running(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// 停止当前实验并返回结果。
    pub fn stop_experiment(&self) -> Option<ChaosExperimentResult> {
        self.enabled.store(false, Ordering::SeqCst);
        let experiment = self.current_experiment.lock().take();
        experiment.map(|exp| {
            let ops = self.op_count.load(Ordering::SeqCst);
            let faults = self.fault_count.load(Ordering::SeqCst);
            ChaosExperimentResult {
                name: exp.name,
                duration: exp.duration,
                operations_attempted: ops,
                operations_succeeded: ops.saturating_sub(faults),
                operations_failed: faults,
                faults_injected: faults,
                system_recovered: true,
                observations: vec![],
            }
        })
    }

    /// 检查混沌是否激活，若激活则可能注入故障。
    /// 需要注入故障时返回 `Some(error)`，正常操作时返回 `None`。
    pub fn maybe_inject(&self, op_name: &str) -> Option<CoreError> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }

        self.op_count.fetch_add(1, Ordering::SeqCst);

        let experiment = self.current_experiment.lock();
        let exp = experiment.as_ref()?;

        // 检查操作是否为目标操作
        if let Some(ref targets) = exp.target_operations {
            if !targets.iter().any(|t| op_name.contains(t)) {
                return None;
            }
        }

        // 概率检查
        let roll = simple_random(
            op_name,
            &format!("op_{}", self.op_count.load(Ordering::Relaxed)),
        );
        if roll > exp.fault_probability {
            return None;
        }

        // 从配置列表中随机选择故障类型
        if exp.fault_types.is_empty() {
            return None;
        }

        let fault_idx = simple_usize_random(op_name, exp.fault_types.len());
        let fault = exp.fault_types[fault_idx];

        self.fault_count.fetch_add(1, Ordering::SeqCst);
        drop(experiment);

        Some(match fault {
            FaultType::LlmTimeout => CoreError::Internal("Chaos: LLM timeout injected".into()),
            FaultType::LlmGarbage => {
                CoreError::Internal("Chaos: LLM garbage response injected".into())
            },
            FaultType::ToolCrash => CoreError::Internal("Chaos: Tool crash injected".into()),
            FaultType::ToolHang => CoreError::Internal("Chaos: Tool hang injected".into()),
            FaultType::StorageWriteFailure => {
                CoreError::Internal("Chaos: Storage write failure injected".into())
            },
            FaultType::StorageReadFailure => {
                CoreError::Internal("Chaos: Storage read failure injected".into())
            },
            FaultType::NetworkPartition => {
                CoreError::Internal("Chaos: Network partition injected".into())
            },
            FaultType::RandomDelay(max_ms) => {
                let delay = simple_u64_random(op_name, max_ms);
                std::thread::sleep(Duration::from_millis(delay));
                return None; // Delay is not an error, just latency
            },
        })
    }
}

// ---------------------------------------------------------------------------
// DAG 边界条件
// ---------------------------------------------------------------------------

/// DAG 边界条件的测试辅助工具。
pub mod dag_test {
    use crate::CoreError;
    use sisyphus_types::{SOPGraph, SOPGraphConfig, WasmSOPNode};
    use std::sync::Arc;

    /// 创建只有一个节点的 DAG。
    pub fn single_node(name: &str, tool_id: &str) -> Result<SOPGraphConfig, CoreError> {
        let mut graph = SOPGraph::new();
        graph.add_node(Arc::new(WasmSOPNode {
            name: name.to_string(),
            tool_id: tool_id.to_string(),
            fuel_limit: 1000,
            executor: Arc::new(super::NoopToolExecutor),
            args: Vec::new(),
        }));
        Ok(SOPGraphConfig::from_graph(&graph)?)
    }

    /// 创建包含 `n` 个节点的线性链 DAG。
    pub fn linear_chain(n: usize) -> Result<SOPGraphConfig, CoreError> {
        let mut graph = SOPGraph::new();
        let mut indices = Vec::with_capacity(n);
        for i in 0..n {
            let idx = graph.add_node(Arc::new(WasmSOPNode {
                name: format!("node_{}", i),
                tool_id: format!("tool_{}", i),
                fuel_limit: 1000,
                executor: Arc::new(super::NoopToolExecutor),
                args: Vec::new(),
            }));
            indices.push(idx);
        }
        for i in 1..n {
            graph.add_edge(indices[i - 1], indices[i]);
        }
        Ok(SOPGraphConfig::from_graph(&graph)?)
    }

    /// 创建菱形 DAG：root -> [left, right] -> merge。
    pub fn diamond() -> Result<SOPGraphConfig, CoreError> {
        let mut graph = SOPGraph::new();
        let root = graph.add_node(Arc::new(WasmSOPNode {
            name: "root".into(),
            tool_id: "t1".into(),
            fuel_limit: 1000,
            executor: Arc::new(super::NoopToolExecutor),
            args: Vec::new(),
        }));
        let left = graph.add_node(Arc::new(WasmSOPNode {
            name: "left".into(),
            tool_id: "t2".into(),
            fuel_limit: 1000,
            executor: Arc::new(super::NoopToolExecutor),
            args: Vec::new(),
        }));
        let right = graph.add_node(Arc::new(WasmSOPNode {
            name: "right".into(),
            tool_id: "t3".into(),
            fuel_limit: 1000,
            executor: Arc::new(super::NoopToolExecutor),
            args: Vec::new(),
        }));
        let merge = graph.add_node(Arc::new(WasmSOPNode {
            name: "merge".into(),
            tool_id: "t4".into(),
            fuel_limit: 1000,
            executor: Arc::new(super::NoopToolExecutor),
            args: Vec::new(),
        }));
        graph.add_edge(root, left);
        graph.add_edge(root, right);
        graph.add_edge(left, merge);
        graph.add_edge(right, merge);
        Ok(SOPGraphConfig::from_graph(&graph)?)
    }

    /// 创建宽 DAG：1 个根节点扇出到 `n` 个并行叶节点。
    pub fn wide_fanout(n: usize) -> Result<SOPGraphConfig, CoreError> {
        let mut graph = SOPGraph::new();
        let root = graph.add_node(Arc::new(WasmSOPNode {
            name: "root".into(),
            tool_id: "t_root".into(),
            fuel_limit: 1000,
            executor: Arc::new(super::NoopToolExecutor),
            args: Vec::new(),
        }));
        for i in 0..n {
            let leaf = graph.add_node(Arc::new(WasmSOPNode {
                name: format!("leaf_{}", i),
                tool_id: format!("t_{}", i),
                fuel_limit: 1000,
                executor: Arc::new(super::NoopToolExecutor),
                args: Vec::new(),
            }));
            graph.add_edge(root, leaf);
        }
        Ok(SOPGraphConfig::from_graph(&graph)?)
    }
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 基于输入字符串的确定性伪随机值，范围 [0.0, 1.0)。
fn simple_random(seed1: &str, seed2: &str) -> f64 {
    let mut hasher = DefaultHasher::new();
    seed1.hash(&mut hasher);
    seed2.hash(&mut hasher);
    let hash = hasher.finish();
    (hash as f64) / (u64::MAX as f64)
}

/// 基于输入字符串的确定性伪随机 usize，范围 [0, max)。
fn simple_usize_random(seed: &str, max: usize) -> usize {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let hash = hasher.finish();
    if max == 0 {
        0
    } else {
        (hash as usize) % max
    }
}

/// 基于输入字符串的确定性伪随机 u64，范围 [0, max)。
fn simple_u64_random(seed: &str, max: u64) -> u64 {
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let hash = hasher.finish();
    if max == 0 {
        0
    } else {
        hash % max
    }
}

use async_trait::async_trait;

/// Noop Tool Executor.
pub struct NoopToolExecutor;

#[async_trait]
impl sisyphus_types::ToolExecutor for NoopToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        _args: Vec<u8>,
        _fuel_limit: u64,
    ) -> Result<Vec<u8>, sisyphus_types::TypesError> {
        Ok(format!("noop:{}", tool_id).into_bytes())
    }
}

#[cfg(test)]
/// Make test store.
pub async fn make_test_store() -> Arc<sisyphus_mem::MfsStore> {
    Arc::new(
        sisyphus_mem::MfsStore::new("sqlite::memory:", &sisyphus_types::CacheConfig::default())
            .await
            .unwrap(),
    )
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sisyphus_types::StateStore;
    use sisyphus_types::ToolExecutor;

    #[tokio::test]
    async fn test_chaos_runner_enable_disable() {
        let runner = ChaosRunner::new();
        assert!(
            runner.maybe_inject("test").is_none(),
            "Should not inject when disabled"
        );

        runner.start_experiment(ChaosExperiment {
            name: "test".into(),
            fault_types: vec![FaultType::ToolCrash],
            fault_probability: 1.0,
            duration: Duration::from_secs(1),
            target_operations: None,
        });

        // 概率为 1.0 时，应始终注入
        let result = runner.maybe_inject("execute");
        assert!(result.is_some(), "Should inject when enabled");

        let exp_result = runner.stop_experiment();
        assert!(exp_result.is_some());
        let exp = exp_result.unwrap();
        assert_eq!(exp.faults_injected, 1);
        assert_eq!(exp.operations_attempted, 1);
    }

    #[tokio::test]
    async fn test_chaos_runner_probability() {
        let runner = ChaosRunner::new();
        runner.start_experiment(ChaosExperiment {
            name: "prob-test".into(),
            fault_types: vec![FaultType::RandomDelay(10)],
            fault_probability: 0.0, // Never inject
            duration: Duration::from_secs(1),
            target_operations: None,
        });

        // 概率为 0.0 时，不应注入
        for _ in 0..100 {
            assert!(
                runner.maybe_inject("test").is_none(),
                "Should not inject with 0 probability"
            );
        }

        let result = runner.stop_experiment().unwrap();
        assert_eq!(result.faults_injected, 0);
        assert_eq!(result.operations_attempted, 100);
    }

    #[tokio::test]
    async fn test_chaos_runner_targeted_operations() {
        let runner = ChaosRunner::new();
        runner.start_experiment(ChaosExperiment {
            name: "targeted".into(),
            fault_types: vec![FaultType::ToolCrash],
            fault_probability: 1.0,
            duration: Duration::from_secs(1),
            target_operations: Some(vec!["crash_me".into()]),
        });

        // 非目标操作不应注入
        assert!(
            runner.maybe_inject("safe_op").is_none(),
            "Should not inject on non-targeted op"
        );
        // 目标操作应注入
        assert!(
            runner.maybe_inject("crash_me").is_some(),
            "Should inject on targeted op"
        );

        let result = runner.stop_experiment().unwrap();
        assert_eq!(result.faults_injected, 1);
        assert_eq!(result.operations_attempted, 2);
    }

    #[tokio::test]
    async fn test_chaos_runner_error_rate() {
        let runner = ChaosRunner::new();
        runner.start_experiment(ChaosExperiment {
            name: "rate".into(),
            fault_types: vec![FaultType::ToolCrash],
            fault_probability: 0.5,
            duration: Duration::from_secs(1),
            target_operations: None,
        });

        let mut total_ops = 0u64;
        let mut total_faults = 0u64;
        for _ in 0..200 {
            if runner.maybe_inject("op").is_some() {
                total_faults += 1;
            }
            total_ops += 1;
        }

        // 概率为 0.5 时，预期约 50% 的故障率
        let rate = total_faults as f64 / total_ops as f64;
        assert!(
            (rate - 0.5).abs() < 0.15,
            "Expected ~50% fault rate, got {}%",
            rate * 100.0
        );

        let result = runner.stop_experiment().unwrap();
        drop(result);
    }

    #[tokio::test]
    async fn test_dag_single_node() {
        let config = dag_test::single_node("solo", "tool_x").unwrap();
        assert_eq!(config.nodes.len(), 1);
        assert_eq!(config.nodes[0].name, "solo");
    }

    #[tokio::test]
    async fn test_dag_linear_chain() {
        let config = dag_test::linear_chain(5).unwrap();
        assert_eq!(config.nodes.len(), 5);
        assert_eq!(config.edges.len(), 4);
    }

    #[tokio::test]
    async fn test_dag_diamond() {
        let config = dag_test::diamond().unwrap();
        assert_eq!(config.nodes.len(), 4);
        assert_eq!(config.edges.len(), 4);
    }

    #[tokio::test]
    async fn test_dag_wide_fanout() {
        let config = dag_test::wide_fanout(100).unwrap();
        assert_eq!(config.nodes.len(), 101); // 1 root + 100 leaves
        assert_eq!(config.edges.len(), 100);
    }

    #[tokio::test]
    async fn test_dag_linear_chain_boundary() {
        // 测试边界情况：空、单个、巨大
        let empty = dag_test::linear_chain(0).unwrap();
        assert_eq!(empty.nodes.len(), 0);
        assert_eq!(empty.edges.len(), 0);

        let single = dag_test::linear_chain(1).unwrap();
        assert_eq!(single.nodes.len(), 1);
        assert_eq!(single.edges.len(), 0);

        let large = dag_test::linear_chain(1000).unwrap();
        assert_eq!(large.nodes.len(), 1000);
        assert_eq!(large.edges.len(), 999);
    }

    #[tokio::test]
    async fn test_storage_concurrent_read_write() {
        let store = make_test_store().await;
        let key = "concurrent_key";
        let value = serde_json::json!({"data": "test"});
        let value2 = value.clone();

        let store1 = store.clone();
        let store2 = store.clone();
        let store3 = store.clone();

        // 并发写入
        let h1 = tokio::spawn(async move {
            store1.set_state_json("proj1", key, value).await.unwrap();
        });
        let h2 = tokio::spawn(async move {
            store2.set_state_json("proj2", key, value2).await.unwrap();
        });
        let h3 = tokio::spawn(async move {
            store3.get_state_json("proj1", key).await.unwrap();
        });

        tokio::try_join!(h1, h2, h3).unwrap();

        // 验证隔离性
        let result = store.get_state_json("proj1", key).await.unwrap();
        assert!(result.is_some(), "Project should have its own state");
    }

    #[tokio::test]
    async fn test_storage_high_concurrency() {
        let store = make_test_store().await;
        let mut handles = Vec::new();

        for i in 0..50 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                let key = format!("key_{}", i);
                let val = serde_json::json!({"idx": i});
                s.set_state_json("project", &key, val).await.unwrap();
                let r = s.get_state_json("project", &key).await.unwrap();
                assert!(r.is_some(), "Key {} should be readable", key);
            }));
        }

        for h in handles {
            h.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_noop_tool_executor() {
        let executor = NoopToolExecutor;
        let result = executor.execute("test_tool", vec![], 100).await.unwrap();
        let output = String::from_utf8(result).unwrap();
        assert_eq!(output, "noop:test_tool");
    }
}
