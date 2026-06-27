use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub passed: bool,
    pub details: String,
    pub duration_secs: f64,
}

pub struct VerificationRunner;

impl Default for VerificationRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl VerificationRunner {
    pub fn new() -> Self {
        Self
    }

    pub fn run_kani(&self, target: &str) -> Result<VerificationReport, String> {
        tracing::warn!(target = %target, "Kani verification not implemented yet");
        Ok(VerificationReport {
            passed: false,
            details: "Kani verification is not implemented yet".into(),
            duration_secs: 0.0,
        })
    }

    pub fn run_loom(&self, target: &str) -> Result<VerificationReport, String> {
        tracing::warn!(target = %target, "Loom verification not implemented yet");
        Ok(VerificationReport {
            passed: false,
            details: "Loom verification is not implemented yet".into(),
            duration_secs: 0.0,
        })
    }

    pub fn run_proptest(&self, target: &str, cases: u32) -> Result<VerificationReport, String> {
        tracing::warn!(target = %target, cases = cases, "Proptest verification not implemented yet");
        Ok(VerificationReport {
            passed: false,
            details: format!(
                "Proptest verification with {} cases is not implemented yet",
                cases
            ),
            duration_secs: 0.0,
        })
    }
}
