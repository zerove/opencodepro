use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 团队配置文件结构 (YAML/TOML)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamConfig {
    /// 团队元信息
    #[serde(default)]
    pub metadata: TeamMetadata,
    /// 全局执行策略
    #[serde(default)]
    pub strategy: ExecutionStrategy,
    /// 代理配置列表
    #[serde(default)]
    pub agents: Vec<AgentConfig>,
    /// 任务依赖图 (可选)
    #[serde(default)]
    pub dependencies: HashMap<String, Vec<String>>,
    /// 全局预算控制
    #[serde(default)]
    pub budget: Option<BudgetConfig>,
}

/// 团队元信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TeamMetadata {
    /// 团队名称
    pub name: Option<String>,
    /// 团队描述
    pub description: Option<String>,
    /// 版本
    pub version: Option<String>,
    /// 作者
    pub author: Option<String>,
}

/// 执行策略
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionStrategy {
    /// 并行执行 (默认)
    #[default]
    Parallel,
    /// 串行执行
    Sequential,
    /// 管道执行 (上一个任务输出作为下一个输入)
    Pipeline,
    /// 竞争执行 (多个代理并行尝试，取最佳结果)
    Competitive,
}

impl std::fmt::Display for ExecutionStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionStrategy::Parallel => write!(f, "parallel"),
            ExecutionStrategy::Sequential => write!(f, "sequential"),
            ExecutionStrategy::Pipeline => write!(f, "pipeline"),
            ExecutionStrategy::Competitive => write!(f, "competitive"),
        }
    }
}

impl std::str::FromStr for ExecutionStrategy {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "parallel" => Ok(ExecutionStrategy::Parallel),
            "sequential" => Ok(ExecutionStrategy::Sequential),
            "pipeline" => Ok(ExecutionStrategy::Pipeline),
            "competitive" => Ok(ExecutionStrategy::Competitive),
            _ => Err(anyhow::anyhow!(
                "未知的执行策略: {s}，支持: parallel, sequential, pipeline, competitive"
            )),
        }
    }
}

/// 单个代理配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    /// 代理 ID (内置代理 ID 或自定义 ID)
    pub id: String,
    /// 显示名称 (可选，覆盖内置)
    pub display_name: Option<String>,
    /// 描述 (可选，覆盖内置)
    pub description: Option<String>,
    /// 工具前缀列表 (可选，覆盖内置)
    pub tool_prefixes: Option<Vec<String>>,
    /// 最大并发数 (可选，覆盖内置)
    pub max_concurrency: Option<usize>,
    /// 自定义系统提示词
    pub system_prompt: Option<String>,
    /// 自定义模型参数
    pub model: Option<ModelConfig>,
    /// 环境变量覆盖
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// 是否启用 (默认 true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 代理权重 (用于竞争模式评分)
    #[serde(default)]
    pub weight: f32,
}

fn default_true() -> bool {
    true
}

/// 模型配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// 模型名称
    pub name: String,
    /// API 端点
    pub endpoint: Option<String>,
    /// API Key (建议通过环境变量注入)
    pub api_key: Option<String>,
    /// 温度
    pub temperature: Option<f32>,
    /// 最大 Token
    pub max_tokens: Option<u32>,
    /// Top-p
    pub top_p: Option<f32>,
}

/// 预算配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetConfig {
    /// 最大总 Token 数
    pub max_total_tokens: Option<u64>,
    /// 最大输入 Token 数
    pub max_input_tokens: Option<u64>,
    /// 最大输出 Token 数
    pub max_output_tokens: Option<u64>,
    /// 最大执行时间 (秒)
    pub max_duration_secs: Option<u64>,
    /// 最大成本 (USD)
    pub max_cost_usd: Option<f64>,
}

impl TeamConfig {
    /// 从文件加载配置 (支持 YAML 和 TOML)
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("读取配置文件: {}", path.display()))?;

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "yaml" | "yml" => serde_yaml::from_str(&content).context("解析 YAML 配置失败"),
            "toml" => toml::from_str(&content).context("解析 TOML 配置失败"),
            _ => {
                // 尝试按 YAML 解析，失败再尝试 TOML
                serde_yaml::from_str(&content)
                    .or_else(|_| toml::from_str(&content))
                    .context("解析配置失败 (尝试 YAML 和 TOML 都失败)")
            }
        }
    }

    /// 合并另一个配置 (CLI 参数优先级高于配置文件)
    pub fn merge(&mut self, other: TeamConfig) {
        if other.metadata.name.is_some() {
            self.metadata.name = other.metadata.name;
        }
        if other.metadata.description.is_some() {
            self.metadata.description = other.metadata.description;
        }
        if other.metadata.version.is_some() {
            self.metadata.version = other.metadata.version;
        }
        if other.metadata.author.is_some() {
            self.metadata.author = other.metadata.author;
        }

        // 策略合并：显式设置的策略优先 (非默认值)
        if other.strategy != ExecutionStrategy::default() {
            self.strategy = other.strategy;
        }

        // 代理配置合并
        for other_agent in other.agents {
            if let Some(existing) = self.agents.iter_mut().find(|a| a.id == other_agent.id) {
                // 合并现有代理配置
                existing.merge(other_agent);
            } else {
                // 新增代理
                self.agents.push(other_agent);
            }
        }

        // 依赖合并
        for (task, deps) in other.dependencies {
            self.dependencies.entry(task).or_default().extend(deps);
        }

        // 预算合并
        if other.budget.is_some() {
            self.budget = other.budget;
        }
    }
}

impl AgentConfig {
    /// 合并另一个代理配置 (非 None 字段覆盖)
    pub fn merge(&mut self, other: AgentConfig) {
        if other.display_name.is_some() {
            self.display_name = other.display_name;
        }
        if other.description.is_some() {
            self.description = other.description;
        }
        if other.tool_prefixes.is_some() {
            self.tool_prefixes = other.tool_prefixes;
        }
        if other.max_concurrency.is_some() {
            self.max_concurrency = other.max_concurrency;
        }
        if other.system_prompt.is_some() {
            self.system_prompt = other.system_prompt;
        }
        if other.model.is_some() {
            self.model = other.model;
        }
        if !other.env.is_empty() {
            self.env.extend(other.env);
        }
        // enabled 和 weight 显式设置时覆盖
        if !other.enabled {
            self.enabled = false;
        }
        if other.weight > 0.0 {
            self.weight = other.weight;
        }
    }

    /// 转换为 AgentInfo (用于注册到 AgentRegistry)
    pub fn to_agent_info(&self) -> crate::agent_registry::AgentInfo {
        crate::agent_registry::AgentInfo {
            display_name: self.display_name.clone().unwrap_or_else(|| self.id.clone()),
            description: self.description.clone().unwrap_or_default(),
            tool_prefixes: self
                .tool_prefixes
                .clone()
                .unwrap_or_else(|| vec![format!("{}:", self.id)]),
            max_concurrency: self.max_concurrency.unwrap_or(4),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_team_config_from_yaml() {
        let yaml = r#"
metadata:
  name: "test-team"
  description: "测试团队"
strategy: "parallel"
agents:
  - id: "librarian"
    max_concurrency: 8
  - id: "custom-agent"
    display_name: "自定义代理"
    description: "自定义描述"
    system_prompt: "你是一个专家"
budget:
  max_total_tokens: 100000
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(yaml.as_bytes()).unwrap();
        file.flush().unwrap();

        let config = TeamConfig::from_file(file.path()).unwrap();
        assert_eq!(config.metadata.name, Some("test-team".to_string()));
        assert_eq!(config.metadata.description, Some("测试团队".to_string()));
        assert_eq!(config.strategy, ExecutionStrategy::Parallel);
        assert_eq!(config.agents.len(), 2);
        assert_eq!(config.agents[0].id, "librarian");
        assert_eq!(config.agents[0].max_concurrency, Some(8));
        assert_eq!(config.agents[1].id, "custom-agent");
        assert_eq!(
            config.agents[1].system_prompt,
            Some("你是一个专家".to_string())
        );
        assert_eq!(
            config.budget.as_ref().unwrap().max_total_tokens,
            Some(100000)
        );
    }

    #[test]
    fn test_team_config_from_toml() {
        let toml = r#"
strategy = "sequential"

[metadata]
name = "toml-team"

[[agents]]
id = "metis"
max_concurrency = 4

[budget]
max_cost_usd = 10.0
"#;
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(toml.as_bytes()).unwrap();
        file.flush().unwrap();

        let config = TeamConfig::from_file(file.path()).unwrap();
        assert_eq!(config.metadata.name, Some("toml-team".to_string()));
        assert_eq!(config.strategy, ExecutionStrategy::Sequential);
        assert_eq!(config.agents[0].max_concurrency, Some(4));
        assert_eq!(config.budget.as_ref().unwrap().max_cost_usd, Some(10.0));
    }

    #[test]
    fn test_team_config_merge() {
        let mut base = TeamConfig {
            metadata: TeamMetadata {
                name: Some("base".into()),
                description: Some("base desc".into()),
                ..Default::default()
            },
            strategy: ExecutionStrategy::Parallel,
            agents: vec![AgentConfig {
                id: "librarian".into(),
                max_concurrency: Some(4),
                ..Default::default()
            }],
            ..Default::default()
        };

        let override_config = TeamConfig {
            metadata: TeamMetadata {
                name: Some("override".into()),
                ..Default::default()
            },
            strategy: ExecutionStrategy::Sequential,
            agents: vec![
                AgentConfig {
                    id: "librarian".into(),
                    max_concurrency: Some(8),
                    system_prompt: Some("custom prompt".into()),
                    ..Default::default()
                },
                AgentConfig {
                    id: "metis".into(),
                    ..Default::default()
                },
            ],
            budget: Some(BudgetConfig {
                max_total_tokens: Some(50000),
                ..Default::default()
            }),
            ..Default::default()
        };

        base.merge(override_config);

        assert_eq!(base.metadata.name, Some("override".into()));
        assert_eq!(base.metadata.description, Some("base desc".into())); // 保留
        assert_eq!(base.strategy, ExecutionStrategy::Sequential);
        assert_eq!(base.agents.len(), 2);
        assert_eq!(base.agents[0].max_concurrency, Some(8)); // 覆盖
        assert_eq!(base.agents[0].system_prompt, Some("custom prompt".into())); // 新增
        assert_eq!(base.agents[1].id, "metis"); // 新增
        assert_eq!(base.budget.as_ref().unwrap().max_total_tokens, Some(50000));
    }
}
