//! 认知仪表盘 — 实时可视化 Sisyphus 认知循环。
//!
//! 本模块将结构化的 ExecutionEvent 数据转换为人类可读的、
//! 格式化的终端输出，使 AI 的"思考"过程可观测。

use sisyphus_types::{CognitiveMetrics, ExecutionEvent};
use std::sync::atomic::{AtomicBool, Ordering};

/// Off by default: `broadcast_event` calls `render` for every event, so a
/// library that printed unconditionally wrote to whatever stdout the embedding
/// process had, with no way to turn it off.
static ENABLED: AtomicBool = AtomicBool::new(false);

pub struct CognitiveDashboard;

impl CognitiveDashboard {
    /// Enable or disable terminal rendering for all instances.
    pub fn set_enabled(enabled: bool) {
        ENABLED.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    /// 以富格式将执行事件渲染到终端。
    pub fn render(event: &ExecutionEvent) {
        if !Self::is_enabled() {
            return;
        }
        match event {
            ExecutionEvent::CognitiveStep {
                step,
                message,
                metrics,
            } => {
                Self::render_cognitive_step(step, message, metrics);
            },
            ExecutionEvent::NodeStarted { node_name } => {
                tracing::info!("  🚀 [NODE START] {}", node_name);
            },
            ExecutionEvent::NodeCompleted {
                node_name, output, ..
            } => {
                tracing::info!("  ✅ [NODE DONE] {}: {}", node_name, output);
            },
            ExecutionEvent::NodeFailed {
                node_name, error, ..
            } => {
                tracing::info!("  ❌ [NODE FAIL] {}: {}", node_name, error);
            },
            ExecutionEvent::ExecutionComplete { result } => {
                tracing::info!("\n🎯 [FINAL RESULT] {}", result);
            },
            ExecutionEvent::CognitiveComplete { report } => {
                tracing::info!("\n🏁 [COGNITIVE LOOP CONVERGED]");
                tracing::info!("📊 Report: {}", report);
            },
            ExecutionEvent::MicroReflection {
                node_name,
                prediction,
                actual,
                decision,
                updates,
                reason,
            } => {
                let icon = match decision.as_str() {
                    "CONTINUE" => "🟢",
                    "ADJUST" => "🟡",
                    "PIVOT" => "🔴",
                    _ => "⚪",
                };
                tracing::info!("  {} [MICR0-REFLECT] Node: {}", icon, node_name);
                tracing::info!("    - 🔮 Expect: {}", prediction);
                tracing::info!("    - 📝 Actual : {}", actual);
                tracing::info!("    - ⚖️ Decision: {}", decision);
                if let Some(u) = updates {
                    tracing::info!("    - 🛠️  Adjustments: {:?}", u);
                }
                if let Some(r) = reason {
                    tracing::info!("    - ⚠️  Pivot Reason: {}", r);
                }
            },
            _ => {},
        }
    }

    fn render_cognitive_step(step: &str, message: &str, metrics: &Option<CognitiveMetrics>) {
        let icon = match step {
            "Predicting" => "🔮",
            "Executing" => "⚙️",
            "Reflecting" => "🤔",
            "Evolving" => "🧬",
            _ => "🔹",
        };

        print!("{} [{:<12}] ", icon, step);

        if let Some(m) = metrics {
            let progress_bar = Self::create_progress_bar(m.progress);
            tracing::info!(
                "{} | Iter: {} | Dev: {:.4} | Conf: {:.2}% | {}",
                progress_bar,
                m.iteration,
                m.deviation,
                m.confidence * 100.0,
                message
            );
        } else {
            tracing::info!("{}", message);
        }
    }

    fn create_progress_bar(progress: f32) -> String {
        let width: usize = 15;
        let clamped_progress = progress.clamp(0.0, 100.0) / 100.0;
        let filled = (clamped_progress * width as f32).round() as usize;
        let empty = width.saturating_sub(filled);

        let mut bar = String::new();
        bar.push('[');
        for _ in 0..filled {
            bar.push('█');
        }
        for _ in 0..empty {
            bar.push('░');
        }
        bar.push(']');

        bar
    }
}
