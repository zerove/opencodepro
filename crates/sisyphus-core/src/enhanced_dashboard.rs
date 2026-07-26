//! Enhanced Cognitive Dashboard — rich TUI-style real-time monitoring.
//!
//! Extends the basic CognitiveDashboard with:
// ! - Session-level metrics aggregation
// ! - Resource usage tracking
// ! - Pattern visualization
// ! - ASCII art progress indicators

use serde::{Deserialize, Serialize};
use sisyphus_types::{CognitiveMetrics, ExecutionEvent};
use std::collections::HashMap;
use std::time::Instant;

/// Session-level metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub total_iterations: usize,
    pub total_nodes_executed: usize,
    pub total_nodes_failed: usize,
    pub total_deviation: f64,
    pub avg_deviation: f64,
    pub total_fuel_consumed: u64,
    pub session_duration_ms: u128,
    pub success_rate: f64,
    pub iteration_history: Vec<IterationSnapshot>,
}

/// Snapshot of a single iteration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationSnapshot {
    pub iteration: usize,
    pub deviation: f64,
    pub nodes_completed: usize,
    pub nodes_failed: usize,
    pub fuel_used: u64,
    pub timestamp_ms: u128,
}

impl Default for SessionMetrics {
    fn default() -> Self {
        Self {
            total_iterations: 0,
            total_nodes_executed: 0,
            total_nodes_failed: 0,
            total_deviation: 0.0,
            avg_deviation: 0.0,
            total_fuel_consumed: 0,
            session_duration_ms: 0,
            success_rate: 1.0,
            iteration_history: Vec::new(),
        }
    }
}

/// Enhanced cognitive dashboard with session tracking
pub struct EnhancedDashboard {
    /// Session start time
    start_time: Instant,
    /// Current iteration metrics
    current_iteration: IterationTracker,
    /// Overall session metrics
    session: SessionMetrics,
    /// Node execution history
    node_history: HashMap<String, Vec<NodeExecution>>,
}

struct IterationTracker {
    iteration: usize,
    nodes_completed: usize,
    nodes_failed: usize,
    fuel_used: u64,
}

impl Default for IterationTracker {
    fn default() -> Self {
        Self {
            iteration: 0,
            nodes_completed: 0,
            nodes_failed: 0,
            fuel_used: 0,
        }
    }
}

/// Record of a node execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeExecution {
    pub node_name: String,
    pub status: String,
    pub duration_ms: u128,
    pub fuel_consumed: u64,
    pub timestamp_ms: u128,
}

impl EnhancedDashboard {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            current_iteration: IterationTracker::default(),
            session: SessionMetrics::default(),
            node_history: HashMap::new(),
        }
    }

    /// Process an execution event and update metrics
    pub fn process_event(&mut self, event: &ExecutionEvent) {
        match event {
            ExecutionEvent::NodeStarted { node_name } => {
                println!("  🚀 [{}] {}", self.timestamp(), node_name);
            }
            ExecutionEvent::NodeCompleted {
                node_name, output, ..
            } => {
                self.current_iteration.nodes_completed += 1;
                self.session.total_nodes_executed += 1;
                self.record_node(node_name, "success");
                println!(
                    "  ✅ [{}] {} → {}",
                    self.timestamp(),
                    node_name,
                    truncate(&output.to_string(), 60)
                );
            }
            ExecutionEvent::NodeFailed {
                node_name, error, ..
            } => {
                self.current_iteration.nodes_failed += 1;
                self.session.total_nodes_failed += 1;
                self.record_node(node_name, "failed");
                println!(
                    "  ❌ [{}] {} ✗ {}",
                    self.timestamp(),
                    node_name,
                    truncate(error, 60)
                );
            }
            ExecutionEvent::CognitiveStep {
                step,
                message,
                metrics,
            } => {
                self.render_enhanced_step(step, message, metrics);
            }
            ExecutionEvent::ExecutionComplete { result } => {
                println!(
                    "\n{}",
                    format!("🎯 FINAL RESULT: {}", truncate(&result.to_string(), 100))
                );
                self.render_session_summary();
            }
            ExecutionEvent::CognitiveComplete { report } => {
                println!("\n{}", "═".repeat(60));
                println!("🏁 COGNITIVE LOOP COMPLETE");
                println!("{}", "═".repeat(60));
                println!("📊 {}", report);
            }
            ExecutionEvent::MicroReflection {
                node_name,
                prediction,
                actual,
                decision,
                ..
            } => {
                let icon = match decision.as_str() {
                    "CONTINUE" => "🟢",
                    "ADJUST" => "🟡",
                    "PIVOT" => "🔴",
                    _ => "⚪",
                };
                println!(
                    "  {} [REFLECT] {}: pred=\"{}\" actual=\"{}\"",
                    icon,
                    node_name,
                    truncate(prediction, 30),
                    truncate(actual, 30)
                );
            }
            _ => {}
        }
    }

    /// Process a cognitive step event with enhanced visualization
    fn render_enhanced_step(
        &mut self,
        step: &str,
        message: &str,
        metrics: &Option<CognitiveMetrics>,
    ) {
        let (icon, color) = match step {
            "Predicting" => ("🔮", "\x1b[36m"), // Cyan
            "Executing" => ("⚙️", "\x1b[33m"),  // Yellow
            "Reflecting" => ("🤔", "\x1b[35m"), // Magenta
            "Evolving" => ("🧬", "\x1b[32m"),   // Green
            _ => ("🔹", "\x1b[0m"),
        };
        let reset = "\x1b[0m";

        if let Some(m) = metrics {
            self.current_iteration.iteration = m.iteration;

            let bar = Self::render_progress_bar(m.progress, 20);
            let deviation_indicator = Self::deviation_indicator(m.deviation);

            println!(
                "{}  {} {}[{:<11}]{} {} │ iter {:>3} │ dev {:.4} {} │ conf {:>5.1}% │ {}",
                color,
                icon,
                reset,
                step,
                color,
                bar,
                m.iteration,
                m.deviation,
                deviation_indicator,
                m.confidence * 100.0,
                truncate(message, 40),
            );
        } else {
            println!(
                "{}  {} {}[{:<11}]{} {}",
                color, icon, reset, step, color, message
            );
        }
    }

    fn record_node(&mut self, node_name: &str, status: &str) {
        let execution = NodeExecution {
            node_name: node_name.to_string(),
            status: status.to_string(),
            duration_ms: 0,
            fuel_consumed: 0,
            timestamp_ms: self.start_time.elapsed().as_millis(),
        };
        self.node_history
            .entry(node_name.to_string())
            .or_default()
            .push(execution);
    }

    fn render_session_summary(&self) {
        let elapsed = self.start_time.elapsed();
        println!("\n{}", "─".repeat(60));
        println!("📈 SESSION SUMMARY");
        println!("{}", "─".repeat(60));
        println!("  ⏱️  Duration:    {:.1}s", elapsed.as_secs_f64());
        println!("  🔄 Iterations:  {}", self.session.total_iterations);
        println!("  ✅ Executed:    {}", self.session.total_nodes_executed);
        println!("  ❌ Failed:      {}", self.session.total_nodes_failed);
        if self.session.total_nodes_executed > 0 {
            let success_rate = 1.0
                - (self.session.total_nodes_failed as f64
                    / self.session.total_nodes_executed as f64);
            println!("  📊 Success:     {:.1}%", success_rate * 100.0);
        }
        println!("  ⛽ Fuel used:   {}", self.current_iteration.fuel_used);

        // Show node execution distribution
        if !self.node_history.is_empty() {
            println!("\n  Node Execution Counts:");
            let mut sorted: Vec<_> = self.node_history.iter().collect();
            sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            for (name, executions) in sorted.iter().take(10) {
                let success = executions.iter().filter(|e| e.status == "success").count();
                let total = executions.len();
                println!("    {:<20} │ {}/{} ✅", truncate(name, 20), success, total);
            }
        }
        println!("{}", "─".repeat(60));
    }

    /// Get current session metrics
    pub fn session_metrics(&self) -> &SessionMetrics {
        &self.session
    }

    fn render_progress_bar(progress: f32, width: usize) -> String {
        let clamped = progress.clamp(0.0, 100.0) / 100.0;
        let filled = (clamped * width as f32).round() as usize;
        let empty = width.saturating_sub(filled);

        let mut bar = String::from("[");
        bar.extend(std::iter::repeat_n('█', filled));
        bar.extend(std::iter::repeat_n('░', empty));
        bar.push(']');
        bar
    }

    fn deviation_indicator(deviation: f64) -> &'static str {
        if deviation < 0.1 {
            "🟢"
        } else if deviation < 0.3 {
            "🟡"
        } else if deviation < 0.5 {
            "🟠"
        } else {
            "🔴"
        }
    }

    fn timestamp(&self) -> String {
        format!("{:.1}s", self.start_time.elapsed().as_secs_f64())
    }
}

impl Default for EnhancedDashboard {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_creation() {
        let dashboard = EnhancedDashboard::new();
        assert_eq!(dashboard.session.total_iterations, 0);
    }

    #[test]
    fn test_progress_bar() {
        let bar = EnhancedDashboard::render_progress_bar(50.0, 10);
        assert!(bar.contains('['));
        assert!(bar.contains(']'));
        assert!(bar.contains('█'));
        assert!(bar.contains('░'));
    }

    #[test]
    fn test_deviation_indicator() {
        assert_eq!(EnhancedDashboard::deviation_indicator(0.05), "🟢");
        assert_eq!(EnhancedDashboard::deviation_indicator(0.2), "🟡");
        assert_eq!(EnhancedDashboard::deviation_indicator(0.4), "🟠");
        assert_eq!(EnhancedDashboard::deviation_indicator(0.6), "🔴");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hell…");
    }
}
