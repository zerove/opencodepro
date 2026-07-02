use anyhow::{Context, Result};
use async_trait::async_trait;
use jsonschema::JSONSchema;
use prettytable::{row, Table};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use sisyphus_tools::metis::LlmClient;
use sisyphus_tools::{
    feedback::FeedbackStore, hook::HookRegistry, BuiltinToolExecutor, DynamicLlmClient,
};
use sisyphus_types::{BudgetMode, Config, ToolExecutor};

// ── Schema Definitions ──────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Criterion {
    id: String,
    description: String,
    #[serde(default = "default_weight")]
    weight: f64,
    check_type: CheckType,
    expected_value: Option<String>,
}

fn default_weight() -> f64 {
    1.0
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
enum CheckType {
    Keyword,
    Regex,
    JsonSchema,
    LlmJudge,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Case {
    id: String,
    agent: String,
    input: Value,
    expected: Value,
    criteria: Vec<Criterion>,
    metadata: Option<Value>,
}

// ── Mock LLM Client ────────────────────────────────────────────

struct MockLlmClient;

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn generate(&self, prompt: &str) -> Result<String, String> {
        self.chat("", prompt, false).await
    }

    async fn chat(&self, _system: &str, user: &str, _json_mode: bool) -> Result<String, String> {
        let response = json!({
            "request": user,
            "summary": format!("Mock response for: {}", user),
            "actionable": true,
            "ambiguities": [],
            "implicit_requirements": [
                {
                    "description": "mock req 1",
                    "rationale": "mock rationale",
                    "confidence": 0.9
                }
            ]
        });
        Ok(serde_json::to_string(&response).unwrap())
    }
}

// ── Eval Runner ─────────────────────────────────────────────────

struct EvalRunner {
    executor: BuiltinToolExecutor,
}

impl EvalRunner {
    fn new() -> Self {
        let api_key = std::env::var("LLM_API_KEY").unwrap_or_else(|_| "mock".to_string());
        let endpoint = std::env::var("LLM_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:8080/chat".to_string());
        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gemma-4-31b-it".to_string());

        let llm_client: Arc<dyn LlmClient> = if api_key == "mock" {
            Arc::new(MockLlmClient)
        } else {
            let cfg = Config {
                llm: sisyphus_types::LlmConfig {
                    api_key,
                    endpoint,
                    model,
                    budget_mode: BudgetMode::default(),
                    agents: std::collections::HashMap::new(),
                    categories: std::collections::HashMap::new(),
                    providers: Vec::new(),
                },
                ..Default::default()
            };
            Arc::new(DynamicLlmClient::new(Arc::new(std::sync::RwLock::new(cfg))))
        };

        Self {
            executor: BuiltinToolExecutor {
                registry: HookRegistry::new(),
                llm_client,
                feedback_store: Arc::new(FeedbackStore::default_dir()),
                bridge_client: None,
            },
        }
    }

    async fn run_case(&self, case: &Case) -> CaseResult {
        let tool_id = match case.agent.to_lowercase().as_str() {
            "metis" => "metis/analyse",
            "momus" => {
                if case.input.get("plan_text").is_some() {
                    "momus/review_plan"
                } else if case.input.get("path").is_some() || case.input.get("content").is_some() {
                    "momus/review_code"
                } else if case.input.get("arch_text").is_some() {
                    "momus/review_arch"
                } else {
                    "momus/review_plan"
                }
            }
            "librarian" => {
                if case.input.get("findings").is_some() {
                    "librarian/synthesise"
                } else if case.input.get("url").is_some() {
                    "librarian/fetch_docs"
                } else if case.input.get("query").is_some() {
                    "librarian/research"
                } else {
                    "librarian/research"
                }
            }
            "scribe" => {
                if case.input.get("context").is_some() {
                    "scribe/generate_adr"
                } else {
                    "scribe/update_docs"
                }
            }
            "security" | "securityoracle" => {
                if case.input.get("code").is_some() {
                    "security/audit_code"
                } else {
                    "security/threat_model"
                }
            }
            _ => "unknown",
        };

        let args = serde_json::to_vec(&case.input).unwrap_or_default();

        let result = self.executor.execute(tool_id, args, 0).await;

        match result {
            Ok(bytes) => {
                let output_str = String::from_utf8_lossy(&bytes).to_string();
                let output_json: Value = serde_json::from_slice(&bytes).unwrap_or(json!({}));

                let mut score = 0.0;
                let mut total_weight = 0.0;
                let mut failed_criteria = Vec::new();

                for crit in &case.criteria {
                    total_weight += crit.weight;
                    if self
                        .evaluate_criterion(crit, &output_str, &output_json)
                        .await
                    {
                        score += crit.weight;
                    } else {
                        failed_criteria.push(crit.id.clone());
                    }
                }

                let final_score = if total_weight > 0.0 {
                    score / total_weight
                } else {
                    1.0
                };
                let passed = failed_criteria.is_empty();

                CaseResult {
                    id: case.id.clone(),
                    passed,
                    score: final_score,
                    failed_criteria,
                    error: None,
                }
            }
            Err(e) => {
                eprintln!("Error executing case {}: {}", case.id, e);
                CaseResult {
                    id: case.id.clone(),
                    passed: false,
                    score: 0.0,
                    failed_criteria: vec!["EXECUTION_ERROR".to_string()],
                    error: Some(e.to_string()),
                }
            }
        }
    }

    async fn evaluate_criterion(
        &self,
        crit: &Criterion,
        output_str: &str,
        output_json: &Value,
    ) -> bool {
        match crit.check_type {
            CheckType::Keyword => {
                if let Some(val) = &crit.expected_value {
                    output_str.contains(val)
                } else {
                    false
                }
            }
            CheckType::Regex => {
                if let Some(val) = &crit.expected_value {
                    match Regex::new(val) { Ok(re) => {
                        re.is_match(output_str)
                    } _ => {
                        false
                    }}
                } else {
                    false
                }
            }
            CheckType::JsonSchema => {
                if let Some(val) = &crit.expected_value {
                    if let Ok(schema_val) = serde_json::from_str::<Value>(val) {
                        if let Ok(compiled) = JSONSchema::compile(&schema_val) {
                            return compiled.is_valid(output_json);
                        }
                    }
                    false
                } else {
                    false
                }
            }
            CheckType::LlmJudge => {
                let system_prompt = "You are a senior engineer judging the correctness of an AI agent's response. \
                                     Compare the Agent Output against the Expected Value. \
                                     Respond with ONLY 'true' if the output satisfies the criteria, or 'false' otherwise.";
                let user_prompt = format!(
                    "Expected: {}\nAgent Output: {}\n\nDoes the output satisfy the expected value? Respond true or false.",
                    crit.expected_value.as_deref().unwrap_or("No explicit expectation provided"),
                    output_str
                );

                if let Ok(res) = self
                    .executor
                    .llm_client
                    .chat(system_prompt, &user_prompt, false)
                    .await
                {
                    res.to_lowercase().contains("true")
                } else {
                    false
                }
            }
        }
    }
}

#[derive(Debug)]
struct CaseResult {
    id: String,
    passed: bool,
    score: f64,
    failed_criteria: Vec<String>,
    #[allow(dead_code)]
    error: Option<String>,
}

// ── Main ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let runner = EvalRunner::new();
    let evals_dir = Path::new("evals");

    let mut all_results = Vec::new();
    let mut total_score = 0.0;
    let mut total_cases = 0;

    // Load cases from all agent directories
    let agents = ["metis", "momus", "librarian", "scribe", "security"];
    for agent in agents {
        let cases_path = evals_dir.join(agent).join("cases.json");
        if !cases_path.exists() {
            continue;
        }

        let content = fs::read_to_string(&cases_path)
            .with_context(|| format!("Failed to read {:?}", cases_path))?;
        let cases: Vec<Case> = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {:?}", cases_path))?;

        for case in cases {
            println!("Running case {}...", case.id);
            let res = runner.run_case(&case).await;
            total_score += res.score;
            total_cases += 1;
            all_results.push(res);
        }
    }

    // Print Report
    let mut table = Table::new();
    table.add_row(row!["Case ID", "Status", "Score", "Failed Criteria"]);

    for res in &all_results {
        let status = if res.passed { "PASS" } else { "FAIL" };
        let failed = res.failed_criteria.join(", ");
        table.add_row(row![res.id, status, format!("{:.2}", res.score), failed]);
    }

    println!("\nEvaluation Report:");
    table.printstd();

    if total_cases > 0 {
        println!(
            "\nFinal Score: {:.2} / 1.0 ({}/{})",
            total_score / total_cases as f64,
            all_results.iter().filter(|r| r.passed).count(),
            total_cases
        );
    } else {
        println!("\nNo cases found to evaluate.");
    }

    Ok(())
}
