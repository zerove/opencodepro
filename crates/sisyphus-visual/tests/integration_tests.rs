//! Integration tests for sisyphus-visual
//! Tests the VisualPerceptionProvider trait implementations and VisualPerceptionManager

use std::sync::Arc;

use sisyphus_tools_llm::DummyLlmClient;
use sisyphus_visual::{
    helpers::{base64_encode, is_png, PNG_MAGIC},
    simulated_visual_state, DetectedElement, HttpVlmProvider, SisyphusVisualProvider,
    VisualAnalysisResult, VisualError, VisualPerceptionManager, VisualPerceptionProvider,
    VlmProviderConfig,
};

#[test]
fn test_visual_error_display() {
    let err = VisualError::ProviderError("test error".into());
    assert!(err.to_string().contains("test error"));

    let err = VisualError::ProcessingError("processing failed".into());
    assert!(err.to_string().contains("processing failed"));

    let err = VisualError::InvalidRequest("invalid".into());
    assert!(err.to_string().contains("invalid"));

    let err = VisualError::HttpError("connection failed".into());
    assert!(err.to_string().contains("connection failed"));

    let err = VisualError::Timeout("timeout".into());
    assert!(err.to_string().contains("timeout"));
}

#[test]
fn test_visual_analysis_result_serialization() {
    let result = VisualAnalysisResult {
        description: "test description".into(),
        detected_elements: vec![
            DetectedElement {
                label: "button".into(),
                bounding_box: [10.0, 20.0, 100.0, 50.0],
                confidence: 0.95,
            },
            DetectedElement {
                label: "input".into(),
                bounding_box: [200.0, 300.0, 150.0, 30.0],
                confidence: 0.85,
            },
        ],
        confidence: 0.9,
    };
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: VisualAnalysisResult = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.description, "test description");
    assert_eq!(deserialized.detected_elements.len(), 2);
    assert_eq!(deserialized.detected_elements[0].label, "button");
    assert_eq!(deserialized.detected_elements[1].label, "input");
}

#[test]
fn test_vlm_provider_config_default() {
    let config = VlmProviderConfig::default();
    assert_eq!(config.model, "gpt-4o");
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.timeout_secs, 30);
    assert_eq!(
        config.endpoint,
        "https://api.openai.com/v1/chat/completions"
    );
}

#[test]
fn test_base64_encode() {
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(b"f"), "Zg==");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_encode(b"foo"), "Zm9v");
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64_encode(b"hello world"), "aGVsbG8gd29ybGQ=");
}

#[test]
fn test_simulated_visual_state_is_deterministic_png() {
    let a = simulated_visual_state("fetch and process");
    let b = simulated_visual_state("fetch and process");
    let c = simulated_visual_state("different goal");

    assert_eq!(a.len(), 1024);
    assert_eq!(a, b); // Same seed = same output
    assert_ne!(a, c); // Different seed = different output
    assert!(is_png(&a)); // Must be valid PNG
}

#[test]
fn test_is_png_validates_correctly() {
    let valid_png = simulated_visual_state("test");
    assert!(is_png(&valid_png));

    let invalid_data = b"not a png";
    assert!(!is_png(invalid_data));

    let too_short = &PNG_MAGIC[..4];
    assert!(!is_png(too_short));

    let mut corrupted = PNG_MAGIC.to_vec();
    corrupted[0] = 0x00;
    assert!(!is_png(&corrupted));
}

#[tokio::test]
async fn test_mock_provider_analyze() {
    let llm = Arc::new(DummyLlmClient);
    let provider = Arc::new(SisyphusVisualProvider::new(llm));

    let result = provider
        .analyze_image(b"test image data", "describe this image")
        .await
        .unwrap();

    assert!(result.description.contains("Simulated"));
    assert!(result.description.contains("describe this image"));
    assert_eq!(result.confidence, 0.9);
    assert!(result.detected_elements.is_empty());
}

#[tokio::test]
async fn test_mock_provider_analyze_visual_action() {
    let llm = Arc::new(DummyLlmClient);
    let provider = Arc::new(SisyphusVisualProvider::new(llm));

    // Test with different image data
    let result = provider
        .analyze_image(b"different image", "find button")
        .await
        .unwrap();

    assert!(result.description.contains("Simulated"));
    assert!(result.description.contains("find button"));
    assert_eq!(result.confidence, 0.9);
}

#[tokio::test]
async fn test_mock_provider_embedding() {
    let llm = Arc::new(DummyLlmClient);
    let provider = Arc::new(SisyphusVisualProvider::new(llm));

    let embedding = provider.get_embedding(b"test image").await.unwrap();
    assert_eq!(embedding.len(), 512);
    assert!(embedding.iter().all(|&v| v == 0.1));
}

#[tokio::test]
async fn test_mock_provider_embedding_consistent() {
    let llm = Arc::new(DummyLlmClient);
    let provider = Arc::new(SisyphusVisualProvider::new(llm));

    let emb1 = provider.get_embedding(b"image1").await.unwrap();
    let emb2 = provider.get_embedding(b"image2").await.unwrap();

    // Mock always returns same vector
    assert_eq!(emb1, emb2);
    assert_eq!(emb1.len(), 512);
}

#[test]
fn test_http_provider_creation() {
    let config = VlmProviderConfig {
        api_key: "test-key".into(),
        endpoint: "https://api.example.com/v1/vision".into(),
        model: "test-model".into(),
        ..Default::default()
    };
    let provider = HttpVlmProvider::new(config).unwrap();
    // Just verify it doesn't panic
    assert!(format!("{:?}", provider).contains("HttpVlmProvider"));
}

#[tokio::test]
async fn test_http_provider_rejects_placeholder_bytes() {
    let config = VlmProviderConfig {
        api_key: "test-key".into(),
        endpoint: "https://api.example.com/v1/vision".into(),
        model: "test-model".into(),
        ..Default::default()
    };
    let provider = HttpVlmProvider::new(config).unwrap();

    // Non-PNG placeholder bytes must short-circuit without any HTTP call
    let embedding = provider.get_embedding(&[0u8; 64]).await.unwrap();
    assert_eq!(embedding.len(), 512);
    assert!(embedding.iter().all(|&v| v == 0.1));

    let analysis = provider
        .analyze_image(&[0u8; 64], "describe")
        .await
        .unwrap();
    assert!(analysis.description.contains("Simulated"));
    assert_eq!(analysis.confidence, 0.9);
}

#[tokio::test]
async fn test_http_provider_accepts_valid_png() {
    let config = VlmProviderConfig {
        api_key: "test-key".into(),
        endpoint: "https://api.example.com/v1/vision".into(),
        model: "test-model".into(),
        ..Default::default()
    };
    let provider = HttpVlmProvider::new(config).unwrap();

    // Valid PNG should attempt HTTP call (will fail without real endpoint, but we test the path)
    let valid_png = simulated_visual_state("test goal");
    assert!(is_png(&valid_png));

    // This will fail because there's no real endpoint, but we can verify the path
    // by checking it doesn't return the simulated response
    let embedding_result = provider.get_embedding(&valid_png).await;
    // Should fail with HTTP error, not return mock vector
    assert!(embedding_result.is_err());
    if let Err(VisualError::HttpError(_)) = embedding_result {
        // Expected - no real server
    } else if let Err(VisualError::ProviderError(_)) = embedding_result {
        // Also acceptable - could be provider error
    } else {
        panic!("Expected HTTP or Provider error for valid PNG");
    }
}

#[tokio::test]
async fn test_visual_perception_manager_visual_analysis() {
    let llm = Arc::new(DummyLlmClient);
    let provider = Arc::new(SisyphusVisualProvider::new(llm.clone()));
    let manager = VisualPerceptionManager::new(provider, llm);

    let result = manager
        .process_visual_node(
            &sisyphus_types::graph::NodeKind::VisualAnalysis {
                prompt: "describe the UI".into(),
                image_source: "screen".into(),
            },
            b"test image",
        )
        .await
        .unwrap();

    // Result should be serialized JSON
    assert!(result.contains("Simulated"));
    assert!(result.contains("describe the UI"));
}

#[tokio::test]
async fn test_visual_perception_manager_visual_action() {
    let llm = Arc::new(DummyLlmClient);
    let provider = Arc::new(SisyphusVisualProvider::new(llm.clone()));
    let manager = VisualPerceptionManager::new(provider, llm);

    let result = manager
        .process_visual_node(
            &sisyphus_types::graph::NodeKind::VisualAction {
                action_type: "click".into(),
                target_selector: "button#submit".into(),
                params: "{}".into(),
            },
            b"test image",
        )
        .await
        .unwrap();

    assert!(result.contains("Simulated"));
    assert!(result.contains("button#submit"));
    assert!(result.contains("click"));
}

#[tokio::test]
async fn test_visual_perception_manager_invalid_node() {
    let llm = Arc::new(DummyLlmClient);
    let provider = Arc::new(SisyphusVisualProvider::new(llm.clone()));
    let manager = VisualPerceptionManager::new(provider, llm);

    let result = manager
        .process_visual_node(
            &sisyphus_types::graph::NodeKind::Test {
                command: "echo test".into(),
                workdir: None,
            },
            b"test image",
        )
        .await;

    assert!(result.is_err());
    if let Err(VisualError::InvalidRequest(msg)) = result {
        assert!(msg.contains("not a visual node"));
    } else {
        panic!("Expected InvalidRequest error");
    }
}

#[tokio::test]
async fn test_visual_perception_manager_embedding() {
    let llm = Arc::new(DummyLlmClient);
    let provider = Arc::new(SisyphusVisualProvider::new(llm.clone()));
    let manager = VisualPerceptionManager::new(provider, llm);

    let embedding = manager.get_embedding(b"test image").await.unwrap();
    assert_eq!(embedding.len(), 512);
    assert!(embedding.iter().all(|&v| v == 0.1));
}

#[tokio::test]
async fn test_mock_provider_multiple_analyses() {
    let llm = Arc::new(DummyLlmClient);
    let provider = Arc::new(SisyphusVisualProvider::new(llm));

    // Test multiple prompts
    let prompts = vec![
        "describe the screen",
        "find the login button",
        "what text is visible",
        "locate the menu",
    ];

    for prompt in prompts {
        let result = provider.analyze_image(b"test", prompt).await.unwrap();
        assert!(result.description.contains("Simulated"));
        assert!(result.description.contains(prompt));
    }
}

#[test]
fn test_detected_element_serialization() {
    let elem = DetectedElement {
        label: "test".into(),
        bounding_box: [1.0, 2.0, 3.0, 4.0],
        confidence: 0.5,
    };

    let json = serde_json::to_string(&elem).unwrap();
    let deserialized: DetectedElement = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.label, "test");
    assert_eq!(deserialized.bounding_box, [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(deserialized.confidence, 0.5);
}

#[test]
fn test_vlm_config_with_custom_values() {
    let config = VlmProviderConfig {
        api_key: "custom-key".into(),
        endpoint: "https://custom.api.com/v1".into(),
        model: "custom-model".into(),
        max_retries: 5,
        timeout_secs: 60,
    };

    assert_eq!(config.api_key, "custom-key");
    assert_eq!(config.endpoint, "https://custom.api.com/v1");
    assert_eq!(config.model, "custom-model");
    assert_eq!(config.max_retries, 5);
    assert_eq!(config.timeout_secs, 60);
}

#[tokio::test]
async fn test_manager_with_http_provider_placeholder() {
    let config = VlmProviderConfig {
        api_key: "test-key".into(),
        endpoint: "https://api.example.com/v1/vision".into(),
        model: "test-model".into(),
        ..Default::default()
    };
    let http_provider = Arc::new(HttpVlmProvider::new(config).unwrap());
    let llm = Arc::new(DummyLlmClient);
    let manager = VisualPerceptionManager::new(http_provider, llm);

    // Test with placeholder bytes - should return simulated response
    let result = manager
        .process_visual_node(
            &sisyphus_types::graph::NodeKind::VisualAnalysis {
                prompt: "test".into(),
                image_source: "screen".into(),
            },
            &[0u8; 64], // placeholder
        )
        .await
        .unwrap();

    assert!(result.contains("Simulated"));
}

#[tokio::test]
async fn test_manager_with_http_provider_png() {
    let config = VlmProviderConfig {
        api_key: "test-key".into(),
        endpoint: "https://api.example.com/v1/vision".into(),
        model: "test-model".into(),
        ..Default::default()
    };
    let http_provider = Arc::new(HttpVlmProvider::new(config).unwrap());
    let llm = Arc::new(DummyLlmClient);
    let manager = VisualPerceptionManager::new(http_provider, llm);

    // Test with valid PNG - should attempt HTTP call
    let valid_png = simulated_visual_state("test goal");
    let result = manager
        .process_visual_node(
            &sisyphus_types::graph::NodeKind::VisualAnalysis {
                prompt: "test".into(),
                image_source: "screen".into(),
            },
            &valid_png,
        )
        .await;

    // Should fail with HTTP error since no real endpoint
    assert!(result.is_err());
    if let Err(VisualError::HttpError(_)) = result {
        // Expected
    } else if let Err(VisualError::ProviderError(_)) = result {
        // Also acceptable
    } else {
        panic!("Expected HTTP/Provider error, got: {:?}", result);
    }
}
