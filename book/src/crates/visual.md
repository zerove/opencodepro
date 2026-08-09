# sisyphus-visual — 视觉感知

视觉 crate 通过图像分析与视觉理解提供多模态认知协同。它定义了 `VisualPerceptionProvider` trait 及外部 VLM（视觉-语言模型）API 的具象实现。

## 核心类型

### VisualPerceptionProvider

所有视觉提供器必须实现的 trait：

```rust
#[async_trait]
pub trait VisualPerceptionProvider: Send + Sync {
    async fn analyze_image(&self, image_data: &[u8], prompt: &str) -> Result<VisualAnalysisResult, VisualError>;
    async fn get_embedding(&self, image_data: &[u8]) -> Result<Vec<f32>, VisualError>;
}
```

### VisualAnalysisResult

图像分析的输出：

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

## 提供器

### HttpVlmProvider

调用 OpenAI 兼容视觉 API 的真实 VLM 提供器：

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

配置选项：
- `api_key` — API 认证 token
- `endpoint` — VLM API URL（OpenAI 兼容）
- `model` — 模型标识（例如 "gpt-4o"、"claude-3-opus"）
- `max_retries` — 失败重试次数
- `timeout_secs` — 请求超时（秒）

### SisyphusVisualProvider

用于测试与开发的模拟提供器：

```rust
let llm_client = Arc::new(DummyLlmClient);
let provider = SisyphusVisualProvider::new(llm_client);
let result = provider.analyze_image(b"test", "describe").await?;
// 返回模拟分析结果
```

## VisualPerceptionManager

根据节点类型路由视觉请求的高阶管理器：

```rust
let manager = VisualPerceptionManager::new(provider, llm_client);

// 处理 VisualAnalysis 节点
let result = manager.process_visual_node(&NodeKind::VisualAnalysis { prompt: "...".into() }, &image_data).await?;

// 处理 VisualAction 节点（元素定位）
let result = manager.process_visual_node(
    &NodeKind::VisualAction {
        action_type: "click".into(),
        target_selector: "#submit-button".into(),
        params: serde_json::json!({}),
    },
    &screenshot_data,
).await?;
```

## 集成

视觉 crate 被 `sisyphus-core` 用于：
- **执行器**（Executor）— SOP 执行期间处理视觉节点
- **执行管理器**（Execution Manager）— 将视觉请求路由到合适的提供器
- **战略管理器**（Strategic Manager）— 使用视觉分析理解目标

## 错误处理

所有错误包装在 `VisualError` 中：

```rust
pub enum VisualError {
    ProviderError(String),   // VLM API 错误
    ProcessingError(String), // 图像处理失败
    InvalidRequest(String),  // 无效的节点类型或参数
    HttpError(String),       // 网络/HTTP 错误
    Timeout(String),         // 请求超时
}
```