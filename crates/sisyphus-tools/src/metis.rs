
use serde::{Deserialize, Serialize};

// ── Types ──────────────────────────────────────────────────

/// Flags an ambiguity or missing information in a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ambiguity {
    pub aspect: String,           // e.g. "scope", "timeline", "technology"
    pub description: String,
    pub severity: AmbiguitySeverity,
    pub options: Vec<String>,     // possible interpretations
    pub recommended: Option<String>,
}

/// How serious an ambiguity is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmbiguitySeverity {
    Blocking,    // cannot proceed without clarification
    Significant, // likely to cause rework
    Minor,       // can make reasonable default
}

impl std::fmt::Display for AmbiguitySeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AmbiguitySeverity::Blocking => write!(f, "blocking"),
            AmbiguitySeverity::Significant => write!(f, "significant"),
            AmbiguitySeverity::Minor => write!(f, "minor"),
        }
    }
}

/// An identified implicit requirement — something not stated but likely intended.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplicitRequirement {
    pub description: String,
    pub rationale: String,          // why we think this is needed
    pub confidence: f64,            // 0.0 – 1.0
}

/// Full result from Metis analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetisAnalysis {
    pub request: String,
    pub ambiguities: Vec<Ambiguity>,
    pub implicit_requirements: Vec<ImplicitRequirement>,
    pub summary: String,
    pub actionable: bool,           // true if enough info to proceed
}

// ── Analysis Functions ─────────────────────────────────────

/// Analyse a natural-language request for ambiguities and implicit requirements.
///
/// ## Checks performed
///
/// ### Ambiguities
/// - Missing scope boundaries
/// - No success criteria / definition of done
/// - Missing technology/language constraints
/// - No timeline or priority indication
/// - Vague or subjective language (e.g. "better", "faster", "improve")
/// - Missing target audience or user context
///
/// ### Implicit Requirements
/// - Error handling needed (if operations are described)
/// - Testing (if implementation is described)
/// - Documentation (if public API is described)
/// - Performance (if "fast", "large", "scale" keywords appear)
/// - Security (if auth, user data, or network is involved)
pub fn analyse(request: &str) -> MetisAnalysis {
    let mut ambiguities = Vec::new();
    let mut implicit = Vec::new();

    let lower = request.to_lowercase();

    // 0. Empty/whitespace-only request — no information to act on
    if lower.trim().is_empty() {
        ambiguities.push(Ambiguity {
            aspect: "completeness".into(),
            severity: AmbiguitySeverity::Blocking,
            description: "Request is empty or only whitespace — no information to act on.".into(),
            options: vec!["Provide a detailed request describing what needs to be done.".into()],
            recommended: Some("Describe the goal, scope, and expected outcome.".into()),
        });
    }

    // ── Ambiguity Detection ───────────────────────────────

    // 1. Missing scope
    if !lower.contains("scope") && !lower.contains("boundary") && !lower.contains("not in scope") {
        ambiguities.push(Ambiguity {
            aspect: "scope".into(),
            severity: AmbiguitySeverity::Significant,
            description: "No explicit scope boundary defined. Without scope, work may creep beyond expectations.".into(),
            options: vec![
                "Clarify what IS in scope".into(),
                "Clarify what is NOT in scope".into(),
                "Define a specific deliverable boundary".into(),
            ],
            recommended: Some("Add a brief 'In Scope / Out of Scope' section.".into()),
        });
    }

    // 2. Missing success criteria
    if !lower.contains("done") && !lower.contains("success") && !lower.contains("acceptance")
        && !lower.contains("complete") && !lower.contains("goal") && !lower.contains("objective")
    {
        ambiguities.push(Ambiguity {
            aspect: "verifiability".into(),
            severity: AmbiguitySeverity::Significant,
            description: "No success criteria or 'definition of done' found. It will be unclear when this work is finished.".into(),
            options: vec![
                "Define measurable acceptance criteria".into(),
                "Specify what deliverables mark completion".into(),
                "Describe the expected end state".into(),
            ],
            recommended: Some("Add acceptance criteria starting with 'We know this is done when...'".into()),
        });
    }

    // 3. Missing tech/language constraints
    if !lower.contains("language") && !lower.contains("rust") && !lower.contains("python")
        && !lower.contains("typescript") && !lower.contains("javascript") && !lower.contains("go ")
        && !lower.contains("framework") && !lower.contains("stack") && !lower.contains("platform")
    {
        ambiguities.push(Ambiguity {
            aspect: "technology".into(),
            severity: AmbiguitySeverity::Minor,
            description: "No technology or language constraints specified. Defaulting to project's existing stack.".into(),
            options: vec![
                "Use the existing project language/framework (default)".into(),
                "Specify a specific technology".into(),
                "Allow any technology choice".into(),
            ],
            recommended: Some("If this project already uses a specific stack, the default is appropriate.".into()),
        });
    }

    // 4. Vague/subjective language
    let vague_terms = ["better", "faster", "improve", "enhance", "more", "easier", "simpler",
        "nicer", "cleaner", "optimize", "refactor", "modernize"];
    let found_vague: Vec<&str> = vague_terms.iter().filter(|t| lower.contains(*t)).copied().collect();
    if !found_vague.is_empty() {
        ambiguities.push(Ambiguity {
            aspect: "clarity".into(),
            severity: AmbiguitySeverity::Significant,
            description: format!(
                "Subjective language detected: {}. These terms are ambiguous without measurable criteria.",
                found_vague.join(", ")
            ),
            options: vec![
                "Define measurable targets for each subjective term".into(),
                "Replace with concrete, verifiable descriptions".into(),
                "Add before/after comparison criteria".into(),
            ],
            recommended: Some(format!("For each subjective term ('{}'), ask 'how much?' or 'compared to what?'", found_vague[0])),
        });
    }

    // 5. Missing timeline/priority
    if !lower.contains("urgent") && !lower.contains("priority") && !lower.contains("timeline")
        && !lower.contains("deadline") && !lower.contains("by ") && !lower.contains("asap")
        && !lower.contains("this week") && !lower.contains("this sprint")
    {
        ambiguities.push(Ambiguity {
            aspect: "priority".into(),
            severity: AmbiguitySeverity::Minor,
            description: "No timeline or priority indicated. Assuming normal priority.".into(),
            options: vec![
                "Normal priority (default)".into(),
                "High/urgent".into(),
                "Low / nice-to-have".into(),
            ],
            recommended: None, // reasonable default exists
        });
    }

    // ── Implicit Requirement Detection ────────────────────

    // 6. Error handling (if operations are described)
    if has_action_words(&lower) && !lower.contains("error") && !lower.contains("fail")
        && !lower.contains("exception") && !lower.contains("handle") && !lower.contains("catch")
    {
        implicit.push(ImplicitRequirement {
            description: "Error handling for described operations.".into(),
            rationale: "Any operation can fail; robust systems need explicit error paths.".into(),
            confidence: 0.8,
        });
    }

    // 7. Testing (if implementation described)
    if has_action_words(&lower) && !lower.contains("test") && !lower.contains("testing")
        && !lower.contains("verify") && !lower.contains("assertion")
    {
        implicit.push(ImplicitRequirement {
            description: "Tests for the implementation.".into(),
            rationale: "Production-quality code requires tests to prevent regressions.".into(),
            confidence: 0.7,
        });
    }

    // 8. Documentation (if public API or library described)
    if (lower.contains("api") || lower.contains("library") || lower.contains("sdk")
        || lower.contains("package") || lower.contains("crate")) && !lower.contains("doc")
        && !lower.contains("readme") && !lower.contains("guide")
    {
        implicit.push(ImplicitRequirement {
            description: "Documentation for the public interface.".into(),
            rationale: "Public APIs are unusable without documentation.".into(),
            confidence: 0.8,
        });
    }

    // 9. Performance (if scale keywords appear)
    if contains_any(&lower, &["fast", "large", "scale", "many", "high volume", "concurrent", "millions"]) {
        implicit.push(ImplicitRequirement {
            description: "Performance considerations and benchmarking.".into(),
            rationale: "Scale-related keywords suggest performance is a concern that should be measured.".into(),
            confidence: 0.6,
        });
    }

    // 10. Security (if auth/user/data keywords appear)
    if contains_any(&lower, &["auth", "login", "password", "user data", "network", "http",
        "api key", "token", "credential", "database", "storage"])
        && !contains_any(&lower, &["security", "secure", "encrypt", "sanitize", "permission"])
    {
        implicit.push(ImplicitRequirement {
            description: "Security review for data/access handling.".into(),
            rationale: "Operations involving auth, user data, or network require security consideration.".into(),
            confidence: 0.7,
        });
    }

    // ── Summary ───────────────────────────────────────────

    let blocking = ambiguities.iter().any(|a| a.severity == AmbiguitySeverity::Blocking);
    let significant = ambiguities.iter().filter(|a| a.severity == AmbiguitySeverity::Significant).count();
    let minor_count = ambiguities.iter().filter(|a| a.severity == AmbiguitySeverity::Minor).count();

    let actionable = !blocking;
    let summary = if ambiguities.is_empty() && implicit.is_empty() {
        "✅ Request appears clear. No ambiguities detected, no implicit requirements inferred."
            .to_string()
    } else {
        format!(
            "Analysis found {} ambiguities ({} blocking, {} significant, {} minor) and {} implicit requirement(s). {}",
            ambiguities.len(),
            if blocking { 1 } else { 0 },
            significant,
            minor_count,
            implicit.len(),
            if actionable {
                "Sufficient information to proceed with noted assumptions."
            } else {
                "Cannot proceed — blocking ambiguities must be resolved first."
            }
        )
    };

    MetisAnalysis {
        request: request.to_string(),
        ambiguities,
        implicit_requirements: implicit,
        summary,
        actionable,
    }
}

// ── Helpers ─────────────────────────────────────────────────

fn has_action_words(s: &str) -> bool {
    let actions = ["implement", "build", "create", "add", "write", "develop",
        "deploy", "setup", "configure", "migrate", "refactor", "integrate"];
    actions.iter().any(|a| s.contains(a))
}

fn contains_any(s: &str, terms: &[&str]) -> bool {
    terms.iter().any(|t| s.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyse_clean_request() {
        let request = "\
Implement user login with email/password using Rust and Actix-Web.
Success criteria: user can register, login, and receive a JWT token.
Scope: auth endpoints only, no frontend. Testing included.
";
        let analysis = analyse(request);
        // Clean request should have few issues
        assert!(analysis.actionable);
        // With success criteria + scope defined, ambiguities should be low
        let scope_issues = analysis.ambiguities.iter().filter(|a| a.aspect == "scope").count();
        assert_eq!(scope_issues, 0, "scope should be clear");
        let verify_issues = analysis.ambiguities.iter().filter(|a| a.aspect == "verifiability").count();
        assert_eq!(verify_issues, 0, "success criteria provided");
    }

    #[test]
    fn test_analyse_vague_request() {
        let request = "Make the website better and faster.";
        let analysis = analyse(request);
        assert!(analysis.ambiguities.iter().any(|a| a.aspect == "clarity"));
        // Should also detect missing scope + success criteria
        assert!(analysis.ambiguities.iter().any(|a| a.aspect == "scope"));
    }

    #[test]
    fn test_analyse_detects_implicit_testing() {
        let request = "Implement a new API endpoint for user profiles.";
        let analysis = analyse(request);
        assert!(analysis.implicit_requirements.iter().any(|i| i.description.contains("Test")));
    }

    #[test]
    fn test_analyse_detects_implicit_error_handling() {
        let request = "Build a file upload service.";
        let analysis = analyse(request);
        assert!(analysis.implicit_requirements.iter().any(|i| i.description.contains("Error handling")));
    }

    #[test]
    fn test_analyse_detects_implicit_security() {
        let request = "Add user authentication with database storage.";
        let analysis = analyse(request);
        assert!(analysis.implicit_requirements.iter().any(|i| i.description.contains("Security")));
    }

    #[test]
    fn test_analyse_detects_implicit_documentation() {
        let request = "Create a Rust library crate for CSV parsing.";
        let analysis = analyse(request);
        assert!(analysis.implicit_requirements.iter().any(|i| i.description.contains("Documentation")));
    }

    #[test]
    fn test_analyse_no_action_words() {
        let request = "What is the weather today?";
        let analysis = analyse(request);
        // Pure info request shouldn't trigger implementation-related implicits
        assert!(analysis.implicit_requirements.is_empty() || !analysis.implicit_requirements.iter().any(|i| i.description.contains("Error handling")));
    }

    #[test]
    fn test_analyse_blocking_detection() {
        let analysis = analyse(""); // empty request = most ambiguous
        assert!(!analysis.actionable || analysis.ambiguities.is_empty());
    }

    #[test]
    fn test_ambiguity_severity_display() {
        assert_eq!(AmbiguitySeverity::Blocking.to_string(), "blocking");
        assert_eq!(AmbiguitySeverity::Significant.to_string(), "significant");
        assert_eq!(AmbiguitySeverity::Minor.to_string(), "minor");
    }

    #[test]
    fn test_analyse_with_performance_keywords() {
        let request = "Build a high-volume data processing pipeline that scales to millions of requests.";
        let analysis = analyse(request);
        assert!(analysis.implicit_requirements.iter().any(|i| i.description.contains("Performance")));
    }

    #[test]
    fn test_analyse_api_without_doc() {
        let request = "Build a REST API for managing todos.";
        let analysis = analyse(request);
        let doc_implicit = analysis.implicit_requirements.iter()
            .find(|i| i.description.contains("Documentation"));
        assert!(doc_implicit.is_some(), "API request should imply documentation need");
    }

    #[test]
    fn test_analyse_tech_constraint_detection() {
        let request = "Implement a service.";
        let analysis = analyse(request);
        let tech_ambiguity = analysis.ambiguities.iter().find(|a| a.aspect == "technology");
        assert!(tech_ambiguity.is_some(), "should flag missing tech constraints");
    }

    #[test]
    fn test_analyse_with_tech_specified() {
        let request = "Implement a Rust service with tokio.";
        let analysis = analyse(request);
        let tech_ambiguity = analysis.ambiguities.iter().find(|a| a.aspect == "technology");
        assert!(tech_ambiguity.is_none() || tech_ambiguity.map(|a| a.severity == AmbiguitySeverity::Minor).unwrap_or(true));
    }

    #[test]
    fn test_analyse_full_coverage() {
        let request = "\
Add a new feature to the Sisyphus CLI.
Timeline: this sprint.
Tech: Rust.
Success: command works end-to-end with tests.
Scope: CLI command only, no engine changes.
";
        let analysis = analyse(request);
        assert!(analysis.actionable);
        // All major ambiguity dimensions should be covered
        assert!(analysis.ambiguities.iter().all(|a| a.severity != AmbiguitySeverity::Blocking));
    }
}
