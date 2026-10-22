//! 执行引擎的性能优化。
//!
//! 提供流水线批处理、输出去重与上下文压缩。

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::NodeOutput;

/// 流水线执行批次 — 将就绪节点分组以并发调度。
pub struct ExecutionBatch {
    pub nodes: Vec<(petgraph::graph::NodeIndex, String)>,
    pub batch_id: u64,
}

/// 跟踪流水线执行指标以支持自适应批处理。
#[derive(Debug, Default)]
pub struct PipelineMetrics {
    pub total_batches: usize,
    pub total_nodes_executed: usize,
    pub avg_batch_size: f64,
    pub total_execution_time_ms: u64,
    pub peak_concurrency: usize,
}

impl PipelineMetrics {
    /// Record batch.
    pub fn record_batch(&mut self, batch_size: usize, duration_ms: u64) {
        self.total_batches += 1;
        self.total_nodes_executed += batch_size;
        self.total_execution_time_ms += duration_ms;
        self.avg_batch_size = self.total_nodes_executed as f64 / self.total_batches as f64;
        self.peak_concurrency = self.peak_concurrency.max(batch_size);
    }
}

/// 输出去重缓存 — 避免重复执行相同的节点计算。
pub struct OutputCache {
    cache: dashmap::DashMap<u64, NodeOutput>,
    max_entries: usize,
    insert_count: std::sync::atomic::AtomicU64,
}

impl OutputCache {
    /// New.
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: dashmap::DashMap::new(),
            max_entries,
            insert_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 计算用于去重的内容哈希。
    pub fn content_hash(node_name: &str, inputs: &HashMap<String, String>) -> u64 {
        let mut hasher = DefaultHasher::new();
        node_name.hash(&mut hasher);
        // 对输入排序以保证哈希确定性
        let mut sorted: Vec<_> = inputs.iter().collect();
        sorted.sort_unstable_by(|a, b| a.0.cmp(b.0));
        for (k, v) in sorted {
            k.hash(&mut hasher);
            v.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// 查找缓存的输出。
    pub fn get(&self, hash: u64) -> Option<NodeOutput> {
        self.cache.get(&hash).map(|o| o.clone())
    }

    /// 存储输出。
    ///
    /// 容量已满时按迭代顺序采样逐出少量条目：只收集溢出数量的受害键，
    /// 不做全量扫描。不保证 LRU 语义，仅保证容量上界。
    /// 逐出回归测试见 `eviction_tests`。
    pub fn insert(&self, hash: u64, output: NodeOutput) {
        if !self.cache.contains_key(&hash) && self.cache.len() >= self.max_entries {
            let overflow = self.cache.len().saturating_sub(self.max_entries) + 1;
            // 先收集再删除：持有分片迭代器的同时对同一分片取写锁会死锁。
            let victims: Vec<u64> = self.cache.iter().take(overflow).map(|e| *e.key()).collect();
            for key in victims {
                self.cache.remove(&key);
            }
        }
        self.insert_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.cache.insert(hash, output);
    }

    /// Len.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Clear.
    pub fn clear(&self) {
        self.cache.clear();
    }
}

/// 基于执行指标的自适应批次大小计算器。
pub fn calculate_optimal_batch_size(
    metrics: &PipelineMetrics,
    available_nodes: usize,
    concurrency_limit: usize,
) -> usize {
    if metrics.total_batches == 0 {
        // 首批：保守
        return available_nodes.min(4).min(concurrency_limit);
    }

    // 自适应：批次快则扩大，慢则缩小
    let avg_time_per_node = if metrics.total_nodes_executed > 0 {
        metrics.total_execution_time_ms as f64 / metrics.total_nodes_executed as f64
    } else {
        100.0 // Default estimate
    };

    let target_batch_size = if avg_time_per_node < 50.0 {
        // 快速节点：更大的批次
        (metrics.avg_batch_size * 1.5) as usize
    } else if avg_time_per_node > 500.0 {
        // 慢速节点：更小的批次
        (metrics.avg_batch_size * 0.7) as usize
    } else {
        // 中等：维持当前
        metrics.avg_batch_size as usize
    };

    target_batch_size.max(1).min(available_nodes).min(concurrency_limit)
}

/// 将就绪节点拆分为最优批次以进行流水线执行。
pub fn create_execution_batches(
    ready_nodes: Vec<(petgraph::graph::NodeIndex, String)>,
    metrics: &PipelineMetrics,
    concurrency_limit: usize,
) -> Vec<ExecutionBatch> {
    if ready_nodes.is_empty() {
        return vec![];
    }

    let batch_size = calculate_optimal_batch_size(metrics, ready_nodes.len(), concurrency_limit);

    let mut batches = Vec::new();

    for (batch_id, chunk) in ready_nodes.chunks(batch_size).enumerate() {
        batches.push(ExecutionBatch {
            nodes: chunk.to_vec(),
            batch_id: batch_id as u64,
        });
    }

    batches
}

/// 上下文值压缩器 — 降低大型上下文条目的内存占用。
pub struct ContextCompressor {
    threshold_bytes: usize,
}

impl ContextCompressor {
    /// New.
    pub fn new(threshold_bytes: usize) -> Self {
        Self { threshold_bytes }
    }

    /// 当值超过阈值时进行压缩。
    /// 返回 (compressed_value, was_compressed)。
    pub fn compress(&self, value: &serde_json::Value) -> (serde_json::Value, bool) {
        let size = estimate_json_size(value);

        if size <= self.threshold_bytes {
            return (value.clone(), false);
        }

        // 对大型字符串值：截断并添加标记
        if let serde_json::Value::String(s) = value {
            if s.len() > self.threshold_bytes {
                // 回退到最近的 UTF-8 字符边界再截断，避免切断多字节字符 panic
                let mut end = self.threshold_bytes;
                while end > 0 && !s.is_char_boundary(end) {
                    end -= 1;
                }
                let truncated = &s[..end];
                return (
                    serde_json::Value::String(format!(
                        "{truncated}...[compressed, original {size} bytes]"
                    )),
                    true,
                );
            }
        }

        // 对大型对象：仅保留前 N 个键
        if let serde_json::Value::Object(map) = value {
            if map.len() > 10 {
                let kept: serde_json::Map<String, serde_json::Value> =
                    map.iter().take(10).map(|(k, v)| (k.clone(), v.clone())).collect();
                return (serde_json::Value::Object(kept), true);
            }
        }

        (value.clone(), false)
    }
}

/// 估算 JSON 值的序列化字节数 — 避免为测量大小而分配完整序列化字符串。
/// 结果为近似值（用于阈值判断），嵌套结构递归累加各部分的近似长度。
fn estimate_json_size(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Null => 4,
        serde_json::Value::Bool(b) => {
            if *b {
                4
            } else {
                5
            }
        },
        // 数字的显示长度即为序列化长度
        serde_json::Value::Number(n) => n.to_string().len(),
        // 字符串需要一对引号
        serde_json::Value::String(s) => s.len() + 2,
        serde_json::Value::Array(items) => {
            2 + items.iter().map(estimate_json_size).sum::<usize>() + items.len().saturating_sub(1)
        },
        // 每个键值对额外计 3 字节：`"k":v` 的引号与冒号
        serde_json::Value::Object(map) => {
            2 + map.iter().map(|(k, v)| k.len() + 3 + estimate_json_size(v)).sum::<usize>()
                + map.len().saturating_sub(1)
        },
    }
}

/// 执行分析器 — 跟踪每节点执行时间以进行优化。
#[derive(Debug, Clone)]
pub struct ExecutionProfile {
    pub node_name: String,
    pub duration_ms: u64,
    pub input_size_bytes: usize,
    pub output_size_bytes: usize,
    pub was_cached: bool,
}

/// Profiler.
pub struct Profiler {
    profiles: Vec<ExecutionProfile>,
    max_profiles: usize,
}

impl Profiler {
    /// New.
    pub fn new(max_profiles: usize) -> Self {
        Self {
            profiles: Vec::with_capacity(max_profiles.min(1000)),
            max_profiles,
        }
    }

    /// Record.
    pub fn record(&mut self, profile: ExecutionProfile) {
        if self.profiles.len() >= self.max_profiles {
            // 仅保留最近 75%
            let keep = self.max_profiles * 3 / 4;
            self.profiles.drain(..self.profiles.len() - keep);
        }
        self.profiles.push(profile);
    }

    /// 获取某节点类型的平均执行时间。
    pub fn avg_duration(&self, node_pattern: &str) -> Option<f64> {
        let matching: Vec<&ExecutionProfile> =
            self.profiles.iter().filter(|p| p.node_name.contains(node_pattern)).collect();

        if matching.is_empty() {
            return None;
        }

        let total: u64 = matching.iter().map(|p| p.duration_ms).sum();
        Some(total as f64 / matching.len() as f64)
    }

    /// 获取缓存命中率。
    pub fn cache_hit_rate(&self) -> f64 {
        if self.profiles.is_empty() {
            return 0.0;
        }
        let hits = self.profiles.iter().filter(|p| p.was_cached).count();
        hits as f64 / self.profiles.len() as f64
    }

    /// 获取最慢的节点。
    pub fn slowest_nodes(&self, n: usize) -> Vec<&ExecutionProfile> {
        let mut sorted: Vec<&ExecutionProfile> = self.profiles.iter().collect();
        sorted.sort_by_key(|b| std::cmp::Reverse(b.duration_ms));
        sorted.into_iter().take(n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_cache_dedup() {
        let cache = OutputCache::new(100);
        let hash =
            OutputCache::content_hash("node1", &HashMap::from([("key".into(), "value".into())]));

        let output = NodeOutput::from(serde_json::Value::String("result".into()));
        cache.insert(hash, output.clone());

        let cached = cache.get(hash);
        assert!(cached.is_some());
    }

    #[test]
    fn test_batch_size_calculation() {
        let mut metrics = PipelineMetrics::default();

        // 首批：保守
        let size = calculate_optimal_batch_size(&metrics, 20, 16);
        assert_eq!(size, 4); // min(20, 4, 16)

        // 快速批次后：扩大
        metrics.record_batch(4, 100); // 25ms per node
        let size = calculate_optimal_batch_size(&metrics, 20, 16);
        assert!(size >= 4);
    }

    #[test]
    fn test_context_compressor() {
        let compressor = ContextCompressor::new(100);

        // 小值：不压缩
        let small = serde_json::Value::String("hello".into());
        let (compressed, was_compressed) = compressor.compress(&small);
        assert!(!was_compressed);
        assert_eq!(compressed, small);

        // 大型字符串：压缩
        let large = serde_json::Value::String("x".repeat(200));
        let (compressed, was_compressed) = compressor.compress(&large);
        assert!(was_compressed);
        if let serde_json::Value::String(s) = compressed {
            assert!(s.len() < 200);
            assert!(s.contains("compressed"));
        }
    }

    #[test]
    fn test_context_compressor_multibyte_truncation() {
        let compressor = ContextCompressor::new(100);
        // "中" 每个占 3 字节：150 字节的字符串在 100 处不是字符边界，
        // 旧实现 `&s[..100]` 会 panic。
        let large = serde_json::Value::String("中".repeat(50));
        let (compressed, was_compressed) = compressor.compress(&large);
        assert!(was_compressed);
        if let serde_json::Value::String(s) = compressed {
            assert!(s.len() < 150);
            assert!(s.contains("compressed"));
        }
    }

    #[test]
    fn test_profiler() {
        let mut profiler = Profiler::new(100);
        profiler.record(ExecutionProfile {
            node_name: "test_node".into(),
            duration_ms: 50,
            input_size_bytes: 100,
            output_size_bytes: 200,
            was_cached: false,
        });

        assert_eq!(profiler.avg_duration("test"), Some(50.0));
        assert_eq!(profiler.cache_hit_rate(), 0.0);
    }
}

#[cfg(test)]
mod eviction_tests {
    use super::*;
    use std::sync::Arc;

    fn output() -> NodeOutput {
        NodeOutput(Arc::new(serde_json::Value::Null))
    }

    /// 回归：逐出必须真的发生。此前 `insert` 删除的是一个用
    /// `evict_target ^ hash` 拼出的键，它几乎必然不存在，缓存因此
    /// 只增不减，`max_entries` 形同虚设。
    #[test]
    fn output_cache_respects_max_entries() {
        let cache = OutputCache::new(10);
        for i in 0..100u64 {
            cache.insert(i, output());
        }
        assert!(
            cache.len() <= 10,
            "cache grew to {} entries, max_entries is 10",
            cache.len()
        );
    }

    #[test]
    fn output_cache_keeps_recently_inserted_entries() {
        let cache = OutputCache::new(10);
        for i in 0..50u64 {
            cache.insert(i, output());
        }
        assert!(cache.get(49).is_some(), "newest entry must survive");
    }
}
