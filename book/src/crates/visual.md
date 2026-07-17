# sisyphus-visual — Visual Perception

The visual crate provides multi-modal cognitive synergy through image analysis and visual understanding. It defines the `VisualPerceptionProvider` trait and concrete implementations for external VLM (Vision-Language Model) APIs.

## Core Types

### VisualPerceptionProvider

Trait that all visual providers must implement:

```rust
#[async_trait]
pub trait VisualPerceptionProvider: Send + Sync {
    async fn analyze_image(&self, image_data: &[u8], prompt: &str) -> Result<VisualAnalysisResult, VisualError>;
    async fn get_embedding(&self, image_data: &[u8]) -> Result<Vec<f32>, VisualError>;
}
```

### VisualAnalysisResult

Output from image analysis:

```rust
pub struct VisualAnalysisResult {
    pub description: String,
    pub detected_elements: Vec<DetectedElement>,
    pub confidence: f32,
}

pub struct DetectedElement {
    pub label: String,
    pub bounding_box: [f32; 4], // [x, y, w, h]
    pub confidence: f32,
}
```

## Providers

### HttpVlmProvider

Real VLM provider that calls OpenAI-compatible Vision APIs:

```rust
let config = VlmProviderConfig {
    api_key: "sk-...".into(),
    endpoint: "https://api.openai.com/v1/chat/completions".into(),
    model: "gpt-4o".into(),
    max_retries: 3,
    timeout_secs: 30,
};

let provider = HttpVlmProvider::new(config);
let result = provider.analyze_image(&image_bytes, "Describe this image").await?;
```

Configuration options:
- `api_key` — API authentication token
- `endpoint` — VLM API URL (OpenAI-compatible)
- `model` — Model identifier (e.g., "gpt-4o", "claude-3-opus")
- `max_retries` — Number of retry attempts on failure
- `timeout_secs` — Request timeout in seconds

### SisyphusVisualProvider

Mock provider for testing and development:

```rust
let llm_client = Arc::new(DummyLlmClient);
let provider = SisyphusVisualProvider::new(llm_client);
let result = provider.analyze_image(b"test", "describe").await?;
// Returns simulated analysis
```

## VisualPerceptionManager

High-level manager that routes visual requests based on node types:

```rust
let manager = VisualPerceptionManager::new(provider, llm_client);

// Process a VisualAnalysis node
let result = manager.process_visual_node(&NodeKind::VisualAnalysis { prompt: "...".into() }, &image_data).await?;

// Process a VisualAction node (element location)
let result = manager.process_visual_node(
    &NodeKind::VisualAction {
        action_type: "click".into(),
        target_selector: "#submit-button".into(),
        params: serde_json::json!({}),
    },
    &screenshot_data,
).await?;
```

## Integration

The visual crate is used by `sisyphus-core` in:
- **Executor** — Processes visual nodes during SOP execution
- **Execution Manager** — Routes visual requests to appropriate providers
- **Strategic Manager** — Uses visual analysis for goal understanding

## Error Handling

All errors are wrapped in `VisualError`:

```rust
pub enum VisualError {
    ProviderError(String),   // VLM API errors
    ProcessingError(String), // Image processing failures
    InvalidRequest(String),  // Invalid node types or parameters
    HttpError(String),       // Network/HTTP errors
    Timeout(String),         // Request timeout
}
```
