//! Intent Gate — classifies user goals into execution modes.
//!
//! Analyzes a goal string to determine whether it should run in
//! `Standard` or `UltraWork` mode.

use std::sync::Arc;

use sisyphus_types::WorkMode;

/// IntentGate analyzes the user's goal to determine the appropriate execution mode.
pub struct IntentGate;

impl IntentGate {
    pub async fn analyze(goal: &str, llm: &Arc<dyn sisyphus_tools_llm::LlmClient>) -> WorkMode {
        let prompt = format!(
            "You are the Intent Gate of Sisyphus, a high-rigor AI orchestration engine. 
            Your task is to classify the user's goal into one of two execution modes: 'Standard' or 'UltraWork'.

            ### Criteria for 'UltraWork' (High-Rigor Mode):
            1. **Architectural Impact**: Any request involving refactoring, designing new modules, or changing core data structures.
            2. **High Criticality**: Bugfixes for production crashes, security vulnerabilities, or data loss issues.
            3. **Deep Investigation**: Requests to 'diagnose', 'trace', or 'find the root cause' of non-obvious behavior.
            4. **Precision Requirements**: Tasks where a 1% error rate is unacceptable (e.g., financial logic, crypto, kernel-level code).
            5. **Explicit Command**: User explicitly mentions 'UltraWork', 'rigorous', 'deep dive', or 'senior engineer mode'.

            ### Criteria for 'Standard' (Balanced Mode):
            1. **Information Retrieval**: Simple questions about how something works or where a file is.
            2. **Trivial Changes**: Typo fixes, documentation updates, or simple variable renaming.
            3. **General Exploration**: Broad queries that don't require a formal plan or verification.

            Goal: \"{}\"

            Return ONLY 'Standard' or 'UltraWork'.",
            goal
        );

        match llm.generate(&prompt).await {
            Ok(res) if res.to_lowercase().contains("ultrawork") => WorkMode::UltraWork,
            _ => WorkMode::Standard,
        }
    }
}
