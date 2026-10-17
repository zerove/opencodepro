use crate::BenchmarkRun;
use crate::ToolResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Comparison of current performance against a historical baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineComparison {
    pub current_duration: f64,
    pub baseline_duration: f64,
    pub regression_pct: f64,
    pub is_significant: bool,
}

/// Persistent benchmark runner that tracks performance trends.
pub struct ContinuousBenchmark {
    storage_path: PathBuf,
    trends: HashMap<String, Vec<BenchmarkRun>>,
}

impl ContinuousBenchmark {
    /// Create a benchmark store at the given directory path.
    pub fn new(storage_dir: &str) -> Self {
        let storage_path = PathBuf::from(storage_dir);
        let trends = Self::load_trends(&storage_path);
        Self {
            storage_path,
            trends,
        }
    }

    /// Create with default path .omo/benchmarks/
    pub fn default_dir() -> Self {
        Self::new(".omo/benchmarks")
    }

    /// Run a benchmark for the given function and record the result.
    pub fn run_benchmark(
        &mut self,
        name: &str,
        f: Box<dyn Fn() + Send>,
        _warmup_iters: u32,
        sample_iters: u32,
    ) -> ToolResult<BenchmarkRun> {
        let _start = Instant::now();

        // Warmup
        for _ in 0..sample_iters {
            f();
        }

        // Timed run
        let run_start = Instant::now();
        for _ in 0..sample_iters {
            f();
        }
        let elapsed = run_start.elapsed();

        let avg_duration = elapsed.as_secs_f64() / f64::from(sample_iters);

        let run = BenchmarkRun {
            name: name.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            duration_secs: avg_duration,
            success: true,
            metrics: serde_json::json!({
                "samples": sample_iters,
                "total_duration_secs": elapsed.as_secs_f64(),
                "warmup": _warmup_iters,
            }),
        };

        self.record_result(run.clone())?;
        Ok(run)
    }

    /// Record a benchmark result to the trend data.
    pub fn record_result(&mut self, run: BenchmarkRun) -> ToolResult<()> {
        self.trends.entry(run.name.clone()).or_default().push(run);
        self.save_trends()
    }

    /// Get the trend data for a benchmark name within a recent window.
    pub fn get_trend(&self, name: &str, window: usize) -> Vec<BenchmarkRun> {
        self.trends
            .get(name)
            .map(|runs| {
                let len = runs.len();
                if len <= window {
                    runs.clone()
                } else {
                    runs[len - window..].to_vec()
                }
            })
            .unwrap_or_default()
    }

    /// Compare the most recent run with the historical baseline (median of first N runs).
    pub fn compare_with_baseline(&self, name: &str) -> Option<BaselineComparison> {
        let runs = self.trends.get(name)?;
        if runs.len() < 2 {
            return None;
        }

        // Baseline: median of all but the last run
        let (baseline_runs, current) = {
            let mut all = runs.clone();
            let current = all.pop()?;
            (all, current)
        };

        if baseline_runs.is_empty() {
            return None;
        }

        let mut durations: Vec<f64> = baseline_runs.iter().map(|r| r.duration_secs).collect();
        durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let baseline_duration = durations[durations.len() / 2]; // median

        let regression_pct = if baseline_duration > 0.0 {
            ((current.duration_secs - baseline_duration) / baseline_duration) * 100.0
        } else {
            0.0
        };

        // Consider >10% regression as significant
        let is_significant = regression_pct > 10.0;

        Some(BaselineComparison {
            current_duration: current.duration_secs,
            baseline_duration,
            regression_pct,
            is_significant,
        })
    }

    /// List all benchmark names being tracked.
    pub fn list_benchmarks(&self) -> Vec<String> {
        self.trends.keys().cloned().collect()
    }

    fn load_trends(path: &PathBuf) -> HashMap<String, Vec<BenchmarkRun>> {
        std::fs::create_dir_all(path).ok();
        let file_path = path.join("benchmarks.json");
        std::fs::read_to_string(&file_path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save_trends(&self) -> ToolResult<()> {
        std::fs::create_dir_all(&self.storage_path).map_err(crate::ToolError::Io)?;
        let file_path = self.storage_path.join("benchmarks.json");
        let content = serde_json::to_string_pretty(&self.trends)
            .map_err(|e| crate::ToolError::Generic(e.to_string()))?;
        std::fs::write(&file_path, content).map_err(crate::ToolError::Io)
    }
}

/// Simple in-process timing utility for ad-hoc benchmarks.
pub struct Timer {
    start: Instant,
}

impl Timer {
    /// Start.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Elapsed.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Elapsed secs.
    pub fn elapsed_secs(&self) -> f64 {
        self.elapsed().as_secs_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_and_record_benchmark() {
        let dir = tempfile::tempdir().unwrap();
        let mut bm = ContinuousBenchmark::new(dir.path().to_str().unwrap());

        let run = bm
            .run_benchmark(
                "test_fn",
                Box::new(|| {
                    let _ = 2 + 2;
                }),
                1,
                10,
            )
            .unwrap();

        assert_eq!(run.name, "test_fn");
        assert!(run.success);
        assert!(run.duration_secs > 0.0);
    }

    #[test]
    fn test_get_trend() {
        let dir = tempfile::tempdir().unwrap();
        let mut bm = ContinuousBenchmark::new(dir.path().to_str().unwrap());

        for _ in 0..5 {
            let run = bm
                .run_benchmark(
                    "trend_test",
                    Box::new(|| std::thread::sleep(Duration::from_micros(1))),
                    1,
                    5,
                )
                .unwrap();
            bm.record_result(run).unwrap();
        }

        let trend = bm.get_trend("trend_test", 3);
        assert_eq!(trend.len(), 3);
    }

    #[test]
    fn test_get_trend_nonexistent() {
        let bm = ContinuousBenchmark::new(".omo/benchmarks_test");
        let trend = bm.get_trend("nonexistent", 5);
        assert!(trend.is_empty());
    }

    #[test]
    fn test_compare_with_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let mut bm = ContinuousBenchmark::new(dir.path().to_str().unwrap());

        // Add baseline runs (fast)
        for _ in 0..4 {
            let run = bm
                .run_benchmark(
                    "regression_test",
                    Box::new(|| std::thread::sleep(Duration::from_micros(1))),
                    1,
                    5,
                )
                .unwrap();
            bm.record_result(run).unwrap();
        }

        // Add a slow run (regression)
        let slow_run = bm
            .run_benchmark(
                "regression_test",
                Box::new(|| std::thread::sleep(Duration::from_millis(10))),
                1,
                5,
            )
            .unwrap();
        bm.record_result(slow_run).unwrap();

        let comparison = bm.compare_with_baseline("regression_test").unwrap();
        assert!(comparison.is_significant);
    }

    #[test]
    fn test_compare_with_baseline_insufficient_data() {
        let dir = tempfile::tempdir().unwrap();
        let bm = ContinuousBenchmark::new(dir.path().to_str().unwrap());
        assert!(bm.compare_with_baseline("new_test").is_none());
    }

    #[test]
    fn test_list_benchmarks() {
        let dir = tempfile::tempdir().unwrap();
        let mut bm = ContinuousBenchmark::new(dir.path().to_str().unwrap());

        bm.run_benchmark("a", Box::new(|| {}), 1, 1).unwrap();
        bm.run_benchmark("b", Box::new(|| {}), 1, 1).unwrap();

        let list = bm.list_benchmarks();
        assert!(list.contains(&"a".to_string()));
        assert!(list.contains(&"b".to_string()));
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start();
        std::thread::sleep(Duration::from_millis(10));
        assert!(timer.elapsed_secs() > 0.0);
    }
}
