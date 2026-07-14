//! Cognitive Dashboard — provides real-time visualization of the Sisyphus cognitive loop.
//!
//! This module transforms structured `ExecutionEvent` data into human-readable,
//! formatted terminal output to allow observability into the AI's "thinking" process.

use sisyphus_types::{ExecutionEvent, CognitiveMetrics};
use std::fmt::Write;

pub struct CognitiveDashboard;

impl CognitiveDashboard {
    /// Renders an execution event to the terminal with rich formatting.
    pub fn render(event: &ExecutionEvent) {
        match event {
            ExecutionEvent::CognitiveStep { step, message, metrics } => {
                Self::render_cognitive_step(step, message, metrics);
            }
            ExecutionEvent::NodeStarted { node_name } => {
                println!("  🚀 [NODE START] {}", node_name);
            }
            ExecutionEvent::NodeCompleted { node_name, output, .. } => {
                println!("  ✅ [NODE DONE] {}: {}", node_name, output);
            }
            ExecutionEvent::NodeFailed { node_name, error, .. } => {
                println!("  ❌ [NODE FAIL] {}: {}", node_name, error);
            }
            ExecutionEvent::ExecutionComplete { result } => {
                println!("\n🎯 [FINAL RESULT] {}", result);
            }
            ExecutionEvent::CognitiveComplete { report } => {
                println!("\n🏁 [COGNITIVE LOOP CONVERGED]");
                println!("📊 Report: {}", report);
            }
            ExecutionEvent::MicroReflection { node_name, prediction, actual, decision, updates, reason } => {
                let icon = match decision.as_str() {
                    "CONTINUE" => "🟢",
                    "ADJUST" => "🟡",
                    "PIVOT" => "🔴",
                    _ => "⚪",
                };
                println!("  {} [MICR0-REFLECT] Node: {}", icon, node_name);
                println!("    - 🔮 Expect: {}", prediction);
                println!("    - 📝 Actual : {}", actual);
                println!("    - ⚖️ Decision: {}", decision);
                if let Some(u) = updates {
                    println!("    - 🛠️  Adjustments: {:?}", u);
                }
                if let Some(r) = reason {
                    println!("    - ⚠️  Pivot Reason: {}", r);
                }
            }
            _ => {}
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
            println!("{} | Iter: {} | Dev: {:.4} | Conf: {:.2}% | {}", 
                progress_bar, 
                m.iteration, 
                m.deviation, 
                m.confidence * 100.0, 
                message
            );
        } else {
            println!("{}", message);
        }
    }

    fn create_progress_bar(progress: f32) -> String {
        let width: usize = 15;
        let clamped_progress = progress.clamp(0.0, 100.0) / 100.0;
        let filled = (clamped_progress * width as f32).round() as usize;
        let empty = width.saturating_sub(filled);
        
        let mut bar = String::new();
        bar.push('[');
        for _ in 0..filled { bar.push('█'); }
        for _ in 0..empty { bar.push('░'); }
        bar.push(']');
        
        bar
    }
}
