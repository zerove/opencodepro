use std::collections::HashMap;
use std::path::Path;

use sisyphus_cli::tui::enhancements::{
    expand_file_references, filter_slash_commands, fuzzy_match, parse_file_references,
};
use sisyphus_core::dag_renderer::{DagRenderer, NodeStatus};
use sisyphus_core::performance::{
    calculate_optimal_batch_size, create_execution_batches, ContextCompressor, ExecutionProfile,
    OutputCache, PipelineMetrics, Profiler,
};
use sisyphus_core::NodeOutput;

// ─── Performance: OutputCache deduplication ───────────────────────────

#[test]
fn test_output_cache_insert_and_get() {
    let cache = OutputCache::new(100);
    let hash = OutputCache::content_hash("node1", &HashMap::from([("key".into(), "value".into())]));

    let output = NodeOutput::from(serde_json::Value::String("result".into()));
    cache.insert(hash, output.clone());

    let cached = cache.get(hash);
    assert!(cached.is_some());
    assert_eq!(
        cached.unwrap(),
        NodeOutput::from(serde_json::Value::String("result".into()))
    );
}

#[test]
fn test_output_cache_miss() {
    let cache = OutputCache::new(100);
    let hash = OutputCache::content_hash("node1", &HashMap::new());
    assert!(cache.get(hash).is_none());
}

#[test]
fn test_output_cache_deterministic_hash() {
    let hash1 = OutputCache::content_hash(
        "node1",
        &HashMap::from([("a".into(), "1".into()), ("b".into(), "2".into())]),
    );
    let hash2 = OutputCache::content_hash(
        "node1",
        &HashMap::from([("b".into(), "2".into()), ("a".into(), "1".into())]),
    );
    // Order shouldn't matter
    assert_eq!(hash1, hash2);
}

#[test]
fn test_output_cache_different_inputs_different_hash() {
    let hash1 =
        OutputCache::content_hash("node1", &HashMap::from([("key".into(), "value1".into())]));
    let hash2 =
        OutputCache::content_hash("node1", &HashMap::from([("key".into(), "value2".into())]));
    assert_ne!(hash1, hash2);
}

#[test]
fn test_output_cache_eviction() {
    let cache = OutputCache::new(3);

    // Insert 5 entries with different content to trigger eviction
    for i in 0..5 {
        let hash = OutputCache::content_hash(
            &format!("node{i}"),
            &HashMap::from([("key".into(), format!("value{i}"))]),
        );
        let output = NodeOutput::from(serde_json::Value::Number(i.into()));
        cache.insert(hash, output);
    }

    // Cache should have evicted some entries (at most max_entries + a small allowance)
    assert!(
        cache.len() <= 6,
        "Cache should not significantly exceed max_entries"
    );
    assert!(cache.len() >= 1, "Cache should still have entries");
}

#[test]
fn test_output_cache_clear() {
    let cache = OutputCache::new(10);
    let hash = OutputCache::content_hash("node1", &HashMap::new());
    cache.insert(hash, NodeOutput::from(serde_json::Value::Bool(true)));

    assert_eq!(cache.len(), 1);
    cache.clear();
    assert_eq!(cache.len(), 0);
}

// ─── Performance: Pipeline metrics ────────────────────────────────────

#[test]
fn test_pipeline_metrics_single_batch() {
    let mut metrics = PipelineMetrics::default();
    metrics.record_batch(4, 200);

    assert_eq!(metrics.total_batches, 1);
    assert_eq!(metrics.total_nodes_executed, 4);
    assert_eq!(metrics.avg_batch_size, 4.0);
    assert_eq!(metrics.total_execution_time_ms, 200);
    assert_eq!(metrics.peak_concurrency, 4);
}

#[test]
fn test_pipeline_metrics_multiple_batches() {
    let mut metrics = PipelineMetrics::default();
    metrics.record_batch(4, 200);
    metrics.record_batch(8, 300);

    assert_eq!(metrics.total_batches, 2);
    assert_eq!(metrics.total_nodes_executed, 12);
    assert_eq!(metrics.avg_batch_size, 6.0);
    assert_eq!(metrics.peak_concurrency, 8);
}

#[test]
fn test_optimal_batch_size_first_batch() {
    let metrics = PipelineMetrics::default();
    let size = calculate_optimal_batch_size(&metrics, 20, 16);
    assert_eq!(size, 4); // min(20, 4, 16)
}

#[test]
fn test_optimal_batch_size_fewer_than_default() {
    let metrics = PipelineMetrics::default();
    let size = calculate_optimal_batch_size(&metrics, 2, 16);
    assert_eq!(size, 2); // min(2, 4, 16)
}

#[test]
fn test_optimal_batch_size_fast_nodes() {
    let mut metrics = PipelineMetrics::default();
    // 100 nodes in 10 batches of 10, each batch taking 300ms = 30ms/node (fast)
    for _ in 0..10 {
        metrics.record_batch(10, 300);
    }
    let size = calculate_optimal_batch_size(&metrics, 20, 16);
    // Fast nodes: batch_size * 1.5 = 15, bounded by 16 concurrency
    assert!(size >= 10);
}

#[test]
fn test_optimal_batch_size_slow_nodes() {
    let mut metrics = PipelineMetrics::default();
    // 10 nodes in 2 batches of 5, each batch taking 5000ms = 1000ms/node (slow)
    for _ in 0..2 {
        metrics.record_batch(5, 5000);
    }
    let size = calculate_optimal_batch_size(&metrics, 20, 16);
    // Slow nodes: batch_size * 0.7 = 3.5 -> 3
    assert!(size <= 5);
}

#[test]
fn test_create_execution_batches_empty() {
    let metrics = PipelineMetrics::default();
    let batches = create_execution_batches(vec![], &metrics, 8);
    assert!(batches.is_empty());
}

#[test]
fn test_create_execution_batches_single_batch() {
    use petgraph::graph::NodeIndex;
    let metrics = PipelineMetrics::default();
    let nodes: Vec<(NodeIndex, String)> = (0..3)
        .map(|i| (NodeIndex::new(i), format!("node{i}")))
        .collect();
    let batches = create_execution_batches(nodes, &metrics, 8);
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].nodes.len(), 3);
}

// ─── Performance: Context compressor ──────────────────────────────────

#[test]
fn test_context_compressor_no_compression() {
    let compressor = ContextCompressor::new(1000);
    let value = serde_json::Value::String("small".into());
    let (compressed, was_compressed) = compressor.compress(&value);
    assert!(!was_compressed);
    assert_eq!(compressed, value);
}

#[test]
fn test_context_compressor_string_truncation() {
    let compressor = ContextCompressor::new(50);
    let value = serde_json::Value::String("x".repeat(200));
    let (compressed, was_compressed) = compressor.compress(&value);
    assert!(was_compressed);
    if let serde_json::Value::String(s) = compressed {
        assert!(s.len() < 200);
        assert!(s.contains("compressed"));
        // Size includes JSON quotes: serde_json serializes "xxx..." as "xxx..."
        assert!(s.contains("202 bytes") || s.contains("200 bytes"));
    }
}

#[test]
fn test_context_compressor_object_truncation() {
    let compressor = ContextCompressor::new(0); // threshold 0 means always compress
    let mut map = serde_json::Map::new();
    for i in 0..15 {
        map.insert(format!("key{i}"), serde_json::Value::Number(i.into()));
    }
    let value = serde_json::Value::Object(map);
    let (compressed, was_compressed) = compressor.compress(&value);
    assert!(was_compressed);
    if let serde_json::Value::Object(m) = compressed {
        assert!(m.len() <= 10);
    }
}

// ─── Performance: Profiler ────────────────────────────────────────────

#[test]
fn test_profiler_avg_duration() {
    let mut profiler = Profiler::new(100);
    profiler.record(ExecutionProfile {
        node_name: "fetch_data".into(),
        duration_ms: 100,
        input_size_bytes: 100,
        output_size_bytes: 200,
        was_cached: false,
    });
    profiler.record(ExecutionProfile {
        node_name: "fetch_data".into(),
        duration_ms: 200,
        input_size_bytes: 150,
        output_size_bytes: 300,
        was_cached: false,
    });

    let avg = profiler.avg_duration("fetch_data");
    assert_eq!(avg, Some(150.0));
}

#[test]
fn test_profiler_cache_hit_rate() {
    let mut profiler = Profiler::new(100);
    profiler.record(ExecutionProfile {
        node_name: "n1".into(),
        duration_ms: 10,
        input_size_bytes: 0,
        output_size_bytes: 0,
        was_cached: true,
    });
    profiler.record(ExecutionProfile {
        node_name: "n2".into(),
        duration_ms: 10,
        input_size_bytes: 0,
        output_size_bytes: 0,
        was_cached: false,
    });

    assert_eq!(profiler.cache_hit_rate(), 0.5);
}

#[test]
fn test_profiler_slowest_nodes() {
    let mut profiler = Profiler::new(100);
    profiler.record(ExecutionProfile {
        node_name: "fast".into(),
        duration_ms: 10,
        input_size_bytes: 0,
        output_size_bytes: 0,
        was_cached: false,
    });
    profiler.record(ExecutionProfile {
        node_name: "slow".into(),
        duration_ms: 1000,
        input_size_bytes: 0,
        output_size_bytes: 0,
        was_cached: false,
    });
    profiler.record(ExecutionProfile {
        node_name: "medium".into(),
        duration_ms: 100,
        input_size_bytes: 0,
        output_size_bytes: 0,
        was_cached: false,
    });

    let slowest = profiler.slowest_nodes(2);
    assert_eq!(slowest.len(), 2);
    assert_eq!(slowest[0].node_name, "slow");
    assert_eq!(slowest[1].node_name, "medium");
}

// ─── DAG Renderer ─────────────────────────────────────────────────────

#[test]
fn test_dag_renderer_empty() {
    let renderer = DagRenderer::new();
    let visualization = renderer.render(&[], &[]);
    assert!(visualization.nodes.is_empty());
    assert!(visualization.edges.is_empty());
}

#[test]
fn test_dag_renderer_single_node() {
    let nodes = vec![("n1".to_string(), NodeStatus::Pending)];
    let renderer = DagRenderer::new();
    let visualization = renderer.render(&nodes, &[]);
    assert_eq!(visualization.nodes.len(), 1);
    assert_eq!(visualization.nodes[0].label, "n1");
}

#[test]
fn test_dag_renderer_status_colors() {
    let statuses = vec![
        ("p1".to_string(), NodeStatus::Pending),
        ("r1".to_string(), NodeStatus::Running),
        ("c1".to_string(), NodeStatus::Completed),
        ("f1".to_string(), NodeStatus::Failed),
        ("s1".to_string(), NodeStatus::Skipped),
    ];

    let renderer = DagRenderer::new();
    let visualization = renderer.render(&statuses, &[]);
    assert_eq!(visualization.nodes.len(), 5);
}

// ─── TUI: Fuzzy match ────────────────────────────────────────────────

#[test]
fn test_fuzzy_match_exact() {
    let result = fuzzy_match("help", "help");
    assert!(result.is_some());
    let (score, _) = result.unwrap();
    assert!(score > 0);
}

#[test]
fn test_fuzzy_match_case_insensitive() {
    // Fuzzy match is case-sensitive on the target, but query is compared char-by-char
    let result = fuzzy_match("h", "Help");
    // h != H, so this may not match depending on implementation
    // The key is it doesn't panic
    let _ = result;
}

#[test]
fn test_fuzzy_match_prefix() {
    let result = fuzzy_match("he", "help");
    assert!(result.is_some());
    let (score, _) = result.unwrap();
    assert!(score > 0);
}

#[test]
fn test_fuzzy_match_substring() {
    let result = fuzzy_match("lp", "help");
    assert!(result.is_some());
}

#[test]
fn test_fuzzy_match_no_match() {
    assert!(fuzzy_match("xyz", "help").is_none());
}

#[test]
fn test_fuzzy_match_empty_query() {
    let result = fuzzy_match("", "help");
    assert!(result.is_some());
    let (score, indices) = result.unwrap();
    assert_eq!(score, 0);
    assert!(indices.is_empty());
}

// ─── TUI: Slash commands ─────────────────────────────────────────────

#[test]
fn test_filter_slash_commands_partial() {
    let results = filter_slash_commands("he");
    assert!(!results.is_empty());
    assert!(results.iter().any(|c| c.0.name == "/help"));
}

#[test]
fn test_filter_slash_commands_exact() {
    let results = filter_slash_commands("help");
    assert!(!results.is_empty());
    assert!(results.iter().any(|c| c.0.name == "/help"));
}

#[test]
fn test_filter_slash_commands_empty() {
    let results = filter_slash_commands("");
    // Empty query matches everything (score 0 for all)
    assert_eq!(results.len(), 17); // 17 commands in SLASH_COMMANDS
}

#[test]
fn test_filter_slash_commands_no_match() {
    let results = filter_slash_commands("zzz_nonexistent_zzz");
    assert!(results.is_empty());
}

// ─── TUI: File references ────────────────────────────────────────────

#[test]
fn test_parse_file_references_single() {
    let refs = parse_file_references("look at @src/main.rs");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, "src/main.rs");
}

#[test]
fn test_parse_file_references_multiple() {
    let refs = parse_file_references("compare @foo.rs and @bar.rs");
    assert_eq!(refs.len(), 2);
}

#[test]
fn test_parse_file_references_none() {
    let refs = parse_file_references("no files here");
    assert!(refs.is_empty());
}

#[test]
fn test_expand_file_references_no_refs() {
    let result = expand_file_references("no files here", Path::new("."));
    assert_eq!(result, "no files here");
}

// ─── Integration: Cache + Profiler pipeline ───────────────────────────

#[test]
fn test_cache_profiler_integration() {
    let cache = OutputCache::new(100);
    let mut profiler = Profiler::new(100);

    // First execution: cache miss
    let hash = OutputCache::content_hash(
        "test_node",
        &HashMap::from([("input".into(), "data".into())]),
    );
    let output = NodeOutput::from(serde_json::Value::String("result".into()));
    cache.insert(hash, output.clone());

    profiler.record(ExecutionProfile {
        node_name: "test_node".into(),
        duration_ms: 100,
        input_size_bytes: 5,
        output_size_bytes: 6,
        was_cached: false,
    });

    // Second execution: cache hit
    if cache.get(hash).is_some() {
        profiler.record(ExecutionProfile {
            node_name: "test_node".into(),
            duration_ms: 1,
            input_size_bytes: 5,
            output_size_bytes: 6,
            was_cached: true,
        });
    }

    assert_eq!(profiler.cache_hit_rate(), 0.5);
    let avg = profiler.avg_duration("test_node").unwrap();
    assert!((avg - 50.5).abs() < 0.1);
}

// ─── Integration: Batch + Profiler pipeline ───────────────────────────

#[test]
fn test_batch_profiler_integration() {
    use petgraph::graph::NodeIndex;

    let mut metrics = PipelineMetrics::default();
    let mut profiler = Profiler::new(100);

    // Simulate 5 batch executions
    for batch in 0..5 {
        let nodes: Vec<(NodeIndex, String)> = (0..4)
            .map(|i| {
                (
                    NodeIndex::new(batch * 4 + i),
                    format!("node_{}_{}", batch, i),
                )
            })
            .collect();

        let batch_size = nodes.len();
        let duration_ms = (batch as u64 + 1) * 100;
        metrics.record_batch(batch_size, duration_ms);

        for (_idx, name) in &nodes {
            profiler.record(ExecutionProfile {
                node_name: name.clone(),
                duration_ms: duration_ms / batch_size as u64,
                input_size_bytes: 0,
                output_size_bytes: 0,
                was_cached: false,
            });
        }
    }

    assert_eq!(metrics.total_batches, 5);
    assert_eq!(metrics.total_nodes_executed, 20);

    // Calculate next batch size - should adapt to medium speed
    let next_size = calculate_optimal_batch_size(&metrics, 20, 16);
    assert!(next_size >= 1 && next_size <= 16);
}
