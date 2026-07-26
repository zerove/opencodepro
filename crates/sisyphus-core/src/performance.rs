//! Performance optimizations for the execution engine.
//!
//! Provides pipeline batching, output deduplication, and context compression.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::NodeOutput;

/// Pipeline execution batch — groups ready nodes for concurrent dispatch.
pub struct ExecutionBatch {
    pub nodes: Vec<(petgraph::graph::NodeIndex, String)>,
    pub batch_id: u64,
}

/// Tracks pipeline execution metrics for adaptive batching.
#[derive(Debug, Default)]
pub struct PipelineMetrics {
    pub total_batches: usize,
    pub total_nodes_executed: usize,
    pub avg_batch_size: f64,
    pub total_execution_time_ms: u64,
    pub peak_concurrency: usize,
}

impl PipelineMetrics {
    pub fn record_batch(&mut self, batch_size: usize, duration_ms: u64) {
        self.total_batches += 1;
        self.total_nodes_executed += batch_size;
        self.total_execution_time_ms += duration_ms;
        self.avg_batch_size =
            self.total_nodes_executed as f64 / self.total_batches as f64;
        self.peak_concurrency = self.peak_concurrency.max(batch_size);
    }
}

/// Output deduplication cache — avoids re-executing identical node computations.
pub struct OutputCache {
    cache: dashmap::DashMap<u64, NodeOutput>,
    max_entries: usize,
}

impl OutputCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: dashmap::DashMap::new(),
            max_entries,
        }
    }

    /// Compute a content hash for deduplication.
    pub fn content_hash(node_name: &str, inputs: &HashMap<String, String>) -> u64 {
        let mut hasher = DefaultHasher::new();
        node_name.hash(&mut hasher);
        // Sort inputs for deterministic hashing
        let mut sorted: Vec<_> = inputs.iter().collect();
        sorted.sort_by_key(|(k, _)| (*k).clone());
        for (k, v) in sorted {
            k.hash(&mut hasher);
            v.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Look up a cached output.
    pub fn get(&self, hash: u64) -> Option<NodeOutput> {
        self.cache.get(&hash).map(|o| o.clone())
    }

    /// Store an output.
    pub fn insert(&self, hash: u64, output: NodeOutput) {
        if self.cache.len() >= self.max_entries {
            // Simple eviction: remove oldest entry
            // In production, use LRU eviction
            if let Some(oldest) = self.cache.iter().next() {
                let key = *oldest.key();
                self.cache.remove(&key);
            }
        }
        self.cache.insert(hash, output);
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn clear(&self) {
        self.cache.clear();
    }
}

/// Adaptive batch size calculator based on execution metrics.
pub fn calculate_optimal_batch_size(
    metrics: &PipelineMetrics,
    available_nodes: usize,
    concurrency_limit: usize,
) -> usize {
    if metrics.total_batches == 0 {
        // First batch: conservative
        return available_nodes.min(4).min(concurrency_limit);
    }

    // Adaptive: scale up if batches are fast, scale down if slow
    let avg_time_per_node = if metrics.total_nodes_executed > 0 {
        metrics.total_execution_time_ms as f64 / metrics.total_nodes_executed as f64
    } else {
        100.0 // Default estimate
    };

    let target_batch_size = if avg_time_per_node < 50.0 {
        // Fast nodes: larger batches
        (metrics.avg_batch_size * 1.5) as usize
    } else if avg_time_per_node > 500.0 {
        // Slow nodes: smaller batches
        (metrics.avg_batch_size * 0.7) as usize
    } else {
        // Medium: maintain current
        metrics.avg_batch_size as usize
    };

    target_batch_size
        .max(1)
        .min(available_nodes)
        .min(concurrency_limit)
}

/// Split ready nodes into optimal batches for pipeline execution.
pub fn create_execution_batches(
    ready_nodes: Vec<(petgraph::graph::NodeIndex, String)>,
    metrics: &PipelineMetrics,
    concurrency_limit: usize,
) -> Vec<ExecutionBatch> {
    if ready_nodes.is_empty() {
        return vec![];
    }

    let batch_size = calculate_optimal_batch_size(
        metrics,
        ready_nodes.len(),
        concurrency_limit,
    );

    let mut batches = Vec::new();
    let mut batch_id = 0;

    for chunk in ready_nodes.chunks(batch_size) {
        batches.push(ExecutionBatch {
            nodes: chunk.to_vec(),
            batch_id,
        });
        batch_id += 1;
    }

    batches
}

/// Context value compressor — reduces memory usage for large context entries.
pub struct ContextCompressor {
    threshold_bytes: usize,
}

impl ContextCompressor {
    pub fn new(threshold_bytes: usize) -> Self {
        Self { threshold_bytes }
    }

    /// Compress a value if it exceeds the threshold.
    /// Returns (compressed_value, was_compressed).
    pub fn compress(&self, value: &serde_json::Value) -> (serde_json::Value, bool) {
        let size = serde_json::to_string(value).map(|s| s.len()).unwrap_or(0);

        if size <= self.threshold_bytes {
            return (value.clone(), false);
        }

        // For large string values, truncate and add marker
        if let serde_json::Value::String(s) = value {
            if s.len() > self.threshold_bytes {
                let truncated = &s[..self.threshold_bytes];
                return (
                    serde_json::Value::String(format!(
                        "{truncated}...[compressed, original {size} bytes]"
                    )),
                    true,
                );
            }
        }

        // For large objects, keep only first N keys
        if let serde_json::Value::Object(map) = value {
            if map.len() > 10 {
                let kept: serde_json::Map<String, serde_json::Value> = map
                    .iter()
                    .take(10)
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                return (
                    serde_json::Value::Object(kept),
                    true,
                );
            }
        }

        (value.clone(), false)
    }
}

/// Execution profiler — tracks per-node execution times for optimization.
#[derive(Debug, Clone)]
pub struct ExecutionProfile {
    pub node_name: String,
    pub duration_ms: u64,
    pub input_size_bytes: usize,
    pub output_size_bytes: usize,
    pub was_cached: bool,
}

pub struct Profiler {
    profiles: Vec<ExecutionProfile>,
    max_profiles: usize,
}

impl Profiler {
    pub fn new(max_profiles: usize) -> Self {
        Self {
            profiles: Vec::with_capacity(max_profiles.min(1000)),
            max_profiles,
        }
    }

    pub fn record(&mut self, profile: ExecutionProfile) {
        if self.profiles.len() >= self.max_profiles {
            // Keep most recent 75%
            let keep = self.max_profiles * 3 / 4;
            self.profiles.drain(..self.profiles.len() - keep);
        }
        self.profiles.push(profile);
    }

    /// Get average execution time for a node type.
    pub fn avg_duration(&self, node_pattern: &str) -> Option<f64> {
        let matching: Vec<&ExecutionProfile> = self
            .profiles
            .iter()
            .filter(|p| p.node_name.contains(node_pattern))
            .collect();

        if matching.is_empty() {
            return None;
        }

        let total: u64 = matching.iter().map(|p| p.duration_ms).sum();
        Some(total as f64 / matching.len() as f64)
    }

    /// Get cache hit rate.
    pub fn cache_hit_rate(&self) -> f64 {
        if self.profiles.is_empty() {
            return 0.0;
        }
        let hits = self.profiles.iter().filter(|p| p.was_cached).count();
        hits as f64 / self.profiles.len() as f64
    }

    /// Get slowest nodes.
    pub fn slowest_nodes(&self, n: usize) -> Vec<&ExecutionProfile> {
        let mut sorted: Vec<&ExecutionProfile> = self.profiles.iter().collect();
        sorted.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));
        sorted.into_iter().take(n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_cache_dedup() {
        let cache = OutputCache::new(100);
        let hash = OutputCache::content_hash("node1", &HashMap::from([("key".into(), "value".into())]));

        let output = NodeOutput::from(serde_json::Value::String("result".into()));
        cache.insert(hash, output.clone());

        let cached = cache.get(hash);
        assert!(cached.is_some());
    }

    #[test]
    fn test_batch_size_calculation() {
        let mut metrics = PipelineMetrics::default();

        // First batch: conservative
        let size = calculate_optimal_batch_size(&metrics, 20, 16);
        assert_eq!(size, 4); // min(20, 4, 16)

        // After fast batches: scale up
        metrics.record_batch(4, 100); // 25ms per node
        let size = calculate_optimal_batch_size(&metrics, 20, 16);
        assert!(size >= 4);
    }

    #[test]
    fn test_context_compressor() {
        let compressor = ContextCompressor::new(100);

        // Small value: no compression
        let small = serde_json::Value::String("hello".into());
        let (compressed, was_compressed) = compressor.compress(&small);
        assert!(!was_compressed);
        assert_eq!(compressed, small);

        // Large string: compression
        let large = serde_json::Value::String("x".repeat(200));
        let (compressed, was_compressed) = compressor.compress(&large);
        assert!(was_compressed);
        if let serde_json::Value::String(s) = compressed {
            assert!(s.len() < 200);
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
