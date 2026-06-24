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
pub async fn update_docs(client: &dyn LlmClient, doc_content: &str, changes: &str) -> Result<String, String> {
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
