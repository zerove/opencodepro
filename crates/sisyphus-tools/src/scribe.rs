use crate::metis::LlmClient;

/// System prompt for the Scribe agent to ensure professional ADR and documentation output.
pub const SCRIBE_SYSTEM_PROMPT: &str = r#"
You are a professional Technical Scribe. Your goal is to transform technical discussions, meeting notes, or chat histories into high-quality, structured Architecture Decision Records (ADRs) and project documentation.

When generating an ADR, you MUST strictly follow the MADR (Markdown Architecture Decision Record) format. 

### MANDATORY STRUCTURE:
You must include these three sections exactly. Failure to include any one of them is a critical structural failure.

1. ## Context
   (Describe the forces at play, the problem being solved, and the constraints.)

2. ## Decision
   (Clearly state the decision made. Explain the 'why' and the trade-offs considered.)

3. ## Consequences
   (Describe the impact of this decision. What are the positives? What are the negatives? What new constraints are introduced?)

### EXAMPLE FORMAT:
# [Title]
* Status: Accepted
* Date: 2026-06-24

## Context
The team is facing X problem...

## Decision
We decided to use Y because...

## Consequences
- Positive: Z is achieved.
- Negative: W is introduced.

STRICT CONSTRAINT: Do NOT omit any of the '##' headers. Do NOT rename them.
Ensure the tone is objective, concise, and professional. Focus on the 'why' more than the 'how'.
"#;

/// Generates a structured ADR from the provided conversation context.
pub async fn generate_adr(client: &dyn LlmClient, context: &str) -> Result<String, String> {
    client.chat(SCRIBE_SYSTEM_PROMPT, context, false).await
}

/// Suggests updates to existing documentation based on provided changes.
pub async fn update_docs(
    client: &dyn LlmClient,
    doc_content: &str,
    changes: &str,
) -> Result<String, String> {
    let prompt = format!(
        "You are a Technical Scribe. Based on the following changes, suggest updates to the existing documentation. 
Provide the output as a diff or a clear 'Search/Replace' block.

Existing Documentation:
---
{}
---

Requested Changes:
---
{}
---

Suggested Updates:",
        doc_content, changes
    );

    client.chat(SCRIBE_SYSTEM_PROMPT, &prompt, false).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metis::DummyLlmClient;

    #[tokio::test]
    async fn test_generate_adr_passes_context() {
        let client = DummyLlmClient;
        let context = "Team decided to use PostgreSQL over MongoDB for strong consistency";
        let result = generate_adr(&client, context).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.contains(context), "response should echo context");
    }

    #[tokio::test]
    async fn test_generate_adr_returns_string() {
        let client = DummyLlmClient;
        let result = generate_adr(&client, "some context").await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.is_empty(), "response should not be empty");
    }

    #[tokio::test]
    async fn test_update_docs_includes_content() {
        let client = DummyLlmClient;
        let doc = "API Reference - Auth Endpoint";
        let changes = "Add rate limiting section";
        let result = update_docs(&client, doc, changes).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        // DummyLlmClient echoes user input in the summary field (JSON-escaped)
        assert!(
            response.contains(doc),
            "response should echo doc_content; got: {response}"
        );
        assert!(
            response.contains(changes),
            "response should echo changes; got: {response}"
        );
    }

    #[tokio::test]
    async fn test_update_docs_empty_doc() {
        let client = DummyLlmClient;
        let result = update_docs(&client, "", "Remove deprecated endpoint").await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(
            response.contains("Remove deprecated endpoint"),
            "response should contain changes even with empty doc"
        );
    }

    #[tokio::test]
    async fn test_generate_adr_empty_context() {
        let client = DummyLlmClient;
        let result = generate_adr(&client, "").await;
        assert!(result.is_ok(), "should handle empty context gracefully");
    }

    #[test]
    fn test_scribe_system_prompt_has_required_sections() {
        assert!(
            SCRIBE_SYSTEM_PROMPT.contains("## Context"),
            "must contain Context header"
        );
        assert!(
            SCRIBE_SYSTEM_PROMPT.contains("## Decision"),
            "must contain Decision header"
        );
        assert!(
            SCRIBE_SYSTEM_PROMPT.contains("## Consequences"),
            "must contain Consequences header"
        );
        assert!(
            SCRIBE_SYSTEM_PROMPT.contains("MADR"),
            "must mention MADR format"
        );
    }

    #[test]
    fn test_scribe_system_prompt_non_empty() {
        assert!(
            !SCRIBE_SYSTEM_PROMPT.is_empty(),
            "system prompt should not be empty"
        );
        assert!(
            SCRIBE_SYSTEM_PROMPT.len() > 200,
            "system prompt should be substantial"
        );
    }

    #[tokio::test]
    async fn test_update_docs_empty_changes() {
        let client = DummyLlmClient;
        let result = update_docs(&client, "Existing doc here", "").await;
        assert!(result.is_ok(), "should handle empty changes gracefully");
    }

    #[tokio::test]
    async fn test_generate_adr_special_chars() {
        let client = DummyLlmClient;
        let context = "Use $P@TH and special chars: `!@#$%^&*()_+-=[]{}|;':\"/<>?";
        let result = generate_adr(&client, context).await;
        assert!(result.is_ok(), "should handle special characters");
        let response = result.unwrap();
        // DummyLlmClient returns JSON with "summary" containing the context
        assert!(
            response.contains("special"),
            "response should relate to context"
        );
        assert!(
            response.contains("$P@TH") || response.contains("analyse"),
            "response should reference context; got: {response}"
        );
    }

    #[tokio::test]
    async fn test_generate_adr_long_context() {
        let client = DummyLlmClient;
        let long_context = "Decision: ".to_string() + &"A".repeat(5000);
        let result = generate_adr(&client, &long_context).await;
        assert!(result.is_ok(), "should handle long context");
    }

    #[tokio::test]
    async fn test_update_docs_long_document() {
        let client = DummyLlmClient;
        let long_doc = "Doc: ".to_string() + &"B".repeat(3000);
        let result = update_docs(&client, &long_doc, "Minor update").await;
        assert!(result.is_ok(), "should handle long document");
    }
}
