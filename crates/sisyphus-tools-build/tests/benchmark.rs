//! Integration tests for sisyphus-tools-build `benchmark` module's
//! pure data types: `BaselineComparison` and the `default_dir` constructor.

use sisyphus_tools_build::benchmark::{BaselineComparison, ContinuousBenchmark};

// ---------------------------------------------------------------------------
// BaselineComparison
// ---------------------------------------------------------------------------

#[test]
fn baseline_comparison_field_access() {
    let b = BaselineComparison {
        current_duration: 1.5,
        baseline_duration: 1.0,
        regression_pct: 50.0,
        is_significant: true,
    };
    assert_eq!(b.current_duration, 1.5);
    assert_eq!(b.baseline_duration, 1.0);
    assert_eq!(b.regression_pct, 50.0);
    assert!(b.is_significant);
}

#[test]
fn baseline_comparison_no_regression() {
    let b = BaselineComparison {
        current_duration: 1.0,
        baseline_duration: 1.0,
        regression_pct: 0.0,
        is_significant: false,
    };
    assert_eq!(b.regression_pct, 0.0);
    assert!(!b.is_significant);
}

#[test]
fn baseline_comparison_improvement_negative_pct() {
    let b = BaselineComparison {
        current_duration: 0.5,
        baseline_duration: 1.0,
        regression_pct: -50.0,
        is_significant: true,
    };
    assert!(b.regression_pct < 0.0);
}

#[test]
fn baseline_comparison_clone() {
    let a = BaselineComparison {
        current_duration: 2.0,
        baseline_duration: 1.0,
        regression_pct: 100.0,
        is_significant: true,
    };
    let b = a.clone();
    assert_eq!(a.current_duration, b.current_duration);
    assert_eq!(a.regression_pct, b.regression_pct);
}

#[test]
fn baseline_comparison_serde() {
    let b = BaselineComparison {
        current_duration: 2.5,
        baseline_duration: 2.0,
        regression_pct: 25.0,
        is_significant: true,
    };
    let j = serde_json::to_string(&b).unwrap();
    let back: BaselineComparison = serde_json::from_str(&j).unwrap();
    assert_eq!(back.current_duration, 2.5);
    assert_eq!(back.baseline_duration, 2.0);
    assert_eq!(back.regression_pct, 25.0);
    assert!(back.is_significant);
}

#[test]
fn baseline_comparison_debug() {
    let b = BaselineComparison {
        current_duration: 1.0,
        baseline_duration: 1.0,
        regression_pct: 0.0,
        is_significant: false,
    };
    let s = format!("{b:?}");
    assert!(s.contains("current_duration"));
    assert!(s.contains("baseline_duration"));
    assert!(s.contains("regression_pct"));
}

// ---------------------------------------------------------------------------
// ContinuousBenchmark
// ---------------------------------------------------------------------------

#[test]
fn continuous_benchmark_default_dir_constructor() {
    // Use a unique tempdir so that load_trends doesn't see stale state
    let id = std::process::id();
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("sisyphus_test_cb_{id}_{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    let cb = ContinuousBenchmark::new(dir.to_str().unwrap());
    // Empty storage → no benchmarks
    let list = cb.list_benchmarks();
    assert!(list.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn continuous_benchmark_get_trend_unknown_name() {
    let id = std::process::id();
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("sisyphus_test_cb_{id}_{nanos}_b"));
    std::fs::create_dir_all(&dir).unwrap();
    let cb = ContinuousBenchmark::new(dir.to_str().unwrap());
    let trend = cb.get_trend("nonexistent", 10);
    assert!(trend.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn continuous_benchmark_compare_with_baseline_unknown() {
    let id = std::process::id();
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("sisyphus_test_cb_{id}_{nanos}_c"));
    std::fs::create_dir_all(&dir).unwrap();
    let cb = ContinuousBenchmark::new(dir.to_str().unwrap());
    assert!(cb.compare_with_baseline("nonexistent").is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
