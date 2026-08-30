//! 集成测试：tools-agent team_config 配置 + 策略解析
//!
//! 覆盖 `sisyphus_tools_agent::team_config` 中的纯数据模块：
//! - `TeamConfig`/`TeamMetadata`/`AgentConfig`/`ModelConfig`/`BudgetConfig` 默认值 + serde
//! - `ExecutionStrategy` 4 个变体 + Display + FromStr

use sisyphus_tools_agent::team_config::{
    AgentConfig, ExecutionStrategy, ModelConfig, TeamConfig, TeamMetadata,
};

// ---------------------------------------------------------------------------
// TeamConfig
// ---------------------------------------------------------------------------

#[test]
fn team_config_default() {
    let c = TeamConfig::default();
    assert_eq!(c.metadata.name, None);
    assert_eq!(c.strategy, ExecutionStrategy::Parallel);
    assert!(c.agents.is_empty());
    assert!(c.dependencies.is_empty());
    assert!(c.budget.is_none());
}

#[test]
fn team_config_clone() {
    let c = TeamConfig::default();
    let c2 = c.clone();
    assert_eq!(c2.strategy, ExecutionStrategy::Parallel);
    assert_eq!(c2.agents.len(), 0);
}

#[test]
fn team_config_serde_round_trip() {
    let c = TeamConfig {
        metadata: TeamMetadata {
            name: Some("alpha".into()),
            description: Some("A team".into()),
            version: Some("1.0".into()),
            author: Some("x".into()),
        },
        strategy: ExecutionStrategy::Sequential,
        agents: vec![AgentConfig {
            id: "a1".into(),
            display_name: Some("Agent 1".into()),
            description: None,
            tool_prefixes: None,
            max_concurrency: Some(4),
            system_prompt: None,
            model: Some(ModelConfig {
                name: "gpt-4".into(),
                endpoint: None,
                api_key: None,
                temperature: Some(0.7),
                max_tokens: Some(2048),
                top_p: None,
            }),
            env: Default::default(),
            enabled: true,
            weight: 1.0,
        }],
        dependencies: Default::default(),
        budget: None,
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: TeamConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.metadata.name, Some("alpha".to_string()));
    assert_eq!(back.strategy, ExecutionStrategy::Sequential);
    assert_eq!(back.agents.len(), 1);
    assert_eq!(back.agents[0].id, "a1");
}

#[test]
fn team_config_serde_default_fields() {
    // All fields have #[serde(default)]; empty JSON should parse
    let json = "{}";
    let c: TeamConfig = serde_json::from_str(json).unwrap();
    assert_eq!(c.strategy, ExecutionStrategy::Parallel);
    assert!(c.agents.is_empty());
}

#[test]
fn team_config_debug() {
    let c = TeamConfig::default();
    let s = format!("{c:?}");
    assert!(s.contains("TeamConfig"));
}

// ---------------------------------------------------------------------------
// TeamMetadata
// ---------------------------------------------------------------------------

#[test]
fn team_metadata_default_all_none() {
    let m = TeamMetadata::default();
    assert!(m.name.is_none());
    assert!(m.description.is_none());
    assert!(m.version.is_none());
    assert!(m.author.is_none());
}

#[test]
fn team_metadata_serde() {
    let m = TeamMetadata {
        name: Some("n".into()),
        description: Some("d".into()),
        version: Some("v".into()),
        author: Some("a".into()),
    };
    let json = serde_json::to_string(&m).unwrap();
    let back: TeamMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, Some("n".to_string()));
    assert_eq!(back.description, Some("d".to_string()));
    assert_eq!(back.version, Some("v".to_string()));
    assert_eq!(back.author, Some("a".to_string()));
}

// ---------------------------------------------------------------------------
// ExecutionStrategy
// ---------------------------------------------------------------------------

#[test]
fn strategy_default_is_parallel() {
    assert_eq!(ExecutionStrategy::default(), ExecutionStrategy::Parallel);
}

#[test]
fn strategy_display() {
    assert_eq!(format!("{}", ExecutionStrategy::Parallel), "parallel");
    assert_eq!(format!("{}", ExecutionStrategy::Sequential), "sequential");
    assert_eq!(format!("{}", ExecutionStrategy::Pipeline), "pipeline");
    assert_eq!(format!("{}", ExecutionStrategy::Competitive), "competitive");
}

#[test]
fn strategy_from_str_all() {
    use std::str::FromStr;
    assert_eq!(
        ExecutionStrategy::from_str("parallel").unwrap(),
        ExecutionStrategy::Parallel
    );
    assert_eq!(
        ExecutionStrategy::from_str("sequential").unwrap(),
        ExecutionStrategy::Sequential
    );
    assert_eq!(
        ExecutionStrategy::from_str("pipeline").unwrap(),
        ExecutionStrategy::Pipeline
    );
    assert_eq!(
        ExecutionStrategy::from_str("competitive").unwrap(),
        ExecutionStrategy::Competitive
    );
}

#[test]
fn strategy_from_str_case_insensitive() {
    use std::str::FromStr;
    assert_eq!(
        ExecutionStrategy::from_str("PARALLEL").unwrap(),
        ExecutionStrategy::Parallel
    );
    assert_eq!(
        ExecutionStrategy::from_str("Sequential").unwrap(),
        ExecutionStrategy::Sequential
    );
}

#[test]
fn strategy_from_str_unknown_errors() {
    use std::str::FromStr;
    assert!(ExecutionStrategy::from_str("unknown").is_err());
    assert!(ExecutionStrategy::from_str("").is_err());
}

#[test]
fn strategy_serde_lowercase() {
    assert_eq!(
        serde_json::to_string(&ExecutionStrategy::Parallel).unwrap(),
        "\"parallel\""
    );
    assert_eq!(
        serde_json::to_string(&ExecutionStrategy::Sequential).unwrap(),
        "\"sequential\""
    );
    assert_eq!(
        serde_json::to_string(&ExecutionStrategy::Pipeline).unwrap(),
        "\"pipeline\""
    );
    assert_eq!(
        serde_json::to_string(&ExecutionStrategy::Competitive).unwrap(),
        "\"competitive\""
    );
}

#[test]
fn strategy_copy_eq() {
    let a = ExecutionStrategy::Pipeline;
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn strategy_distinct() {
    assert_ne!(ExecutionStrategy::Parallel, ExecutionStrategy::Sequential);
    assert_ne!(ExecutionStrategy::Pipeline, ExecutionStrategy::Competitive);
}

// ---------------------------------------------------------------------------
// AgentConfig
// ---------------------------------------------------------------------------

#[test]
fn agent_config_default() {
    let a = AgentConfig::default();
    assert_eq!(a.id, "");
    assert!(a.display_name.is_none());
    assert!(a.description.is_none());
    assert!(a.tool_prefixes.is_none());
    assert!(a.max_concurrency.is_none());
    assert!(a.system_prompt.is_none());
    assert!(a.model.is_none());
    assert!(a.env.is_empty());
    assert!(!a.enabled); // bool::default() is false
    assert_eq!(a.weight, 0.0);
    // Note: Default::default() for bool is false; the #[serde(default = "default_true")]
    // only applies during deserialization, not Default trait.
}

#[test]
fn agent_config_serde() {
    let a = AgentConfig {
        id: "id1".into(),
        display_name: Some("Name".into()),
        description: Some("Desc".into()),
        tool_prefixes: Some(vec!["tool_".into()]),
        max_concurrency: Some(8),
        system_prompt: Some("You are X".into()),
        model: None,
        env: Default::default(),
        enabled: false,
        weight: 0.5,
    };
    let json = serde_json::to_string(&a).unwrap();
    let back: AgentConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "id1");
    assert_eq!(back.display_name, Some("Name".to_string()));
    assert_eq!(back.tool_prefixes, Some(vec!["tool_".to_string()]));
    assert_eq!(back.max_concurrency, Some(8));
    assert!(!back.enabled);
    assert!((back.weight - 0.5).abs() < 1e-9);
}

#[test]
fn agent_config_serde_default_enabled_true() {
    // Missing "enabled" → default true
    let json = r#"{"id":"a"}"#;
    let a: AgentConfig = serde_json::from_str(json).unwrap();
    assert!(a.enabled);
}

// ---------------------------------------------------------------------------
// ModelConfig
// ---------------------------------------------------------------------------

#[test]
fn model_config_field_access() {
    let m = ModelConfig {
        name: "claude-opus".into(),
        endpoint: Some("https://api.x.com".into()),
        api_key: Some("secret".into()),
        temperature: Some(0.5),
        max_tokens: Some(4096),
        top_p: None,
    };
    assert_eq!(m.name, "claude-opus");
    assert_eq!(m.endpoint, Some("https://api.x.com".to_string()));
    assert_eq!(m.api_key, Some("secret".to_string()));
    assert_eq!(m.temperature, Some(0.5));
    assert_eq!(m.max_tokens, Some(4096));
}

#[test]
fn model_config_serde() {
    let m = ModelConfig {
        name: "gpt-4".into(),
        endpoint: None,
        api_key: None,
        temperature: Some(0.7),
        max_tokens: None,
        top_p: None,
    };
    let json = serde_json::to_string(&m).unwrap();
    let back: ModelConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "gpt-4");
    assert_eq!(back.temperature, Some(0.7));
    assert!(back.endpoint.is_none());
}
