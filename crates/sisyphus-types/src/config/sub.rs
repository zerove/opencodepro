use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::errors::TypesError;

// ── CognitiveLoopConfig ─────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CognitiveLoopConfig {
    pub max_iterations: u32,
    pub convergence_threshold: f64,
}

fn default_cognitive_loop() -> CognitiveLoopConfig {
    CognitiveLoopConfig {
        max_iterations: 10,
        convergence_threshold: 0.15,
    }
}

impl Default for CognitiveLoopConfig {
    fn default() -> Self {
        default_cognitive_loop()
    }
}

impl CognitiveLoopConfig {
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.max_iterations == 0 {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.max_iterations must be > 0".into(),
            ));
        }
        if self.max_iterations > 1000 {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.max_iterations must be ≤ 1000".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.convergence_threshold) {
            return Err(TypesError::ConfigValidationError(
                "cognitive_loop.convergence_threshold must be between 0.0 and 1.0".into(),
            ));
        }
        Ok(())
    }
}

// ── ExecutorConfig ─────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExecutorConfig {
    pub max_concurrent_tasks: usize,
    pub default_fuel: u64,
}

fn default_executor() -> ExecutorConfig {
    ExecutorConfig {
        max_concurrent_tasks: 16,
        default_fuel: 1_000_000,
    }
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        default_executor()
    }
}

impl ExecutorConfig {
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.max_concurrent_tasks == 0 {
            return Err(TypesError::ConfigValidationError(
                "executor.max_concurrent_tasks must be > 0".into(),
            ));
        }
        if self.default_fuel == 0 {
            return Err(TypesError::ConfigValidationError(
                "executor.default_fuel must be > 0".into(),
            ));
        }
        Ok(())
    }
}

// ── CacheConfig ────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CacheConfig {
    pub l1_capacity: usize,
    pub sqlite_path: String,
}

fn default_cache() -> CacheConfig {
    CacheConfig {
        l1_capacity: 10_000,
        sqlite_path: "data/sisyphus.db".to_string(),
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        default_cache()
    }
}

impl CacheConfig {
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.l1_capacity == 0 {
            return Err(TypesError::ConfigValidationError(
                "cache.l1_capacity must be > 0".into(),
            ));
        }
        if self.sqlite_path.is_empty() {
            return Err(TypesError::ConfigValidationError(
                "cache.sqlite_path must not be empty".into(),
            ));
        }
        Ok(())
    }
}

// ── MetricsConfig ──────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MetricsConfig {
    pub prometheus_port: u16,
}

fn default_metrics() -> MetricsConfig {
    MetricsConfig {
        prometheus_port: 9100,
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        default_metrics()
    }
}

impl MetricsConfig {
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.prometheus_port == 0 {
            return Err(TypesError::ConfigValidationError(
                "metrics.prometheus_port must be > 0".into(),
            ));
        }
        Ok(())
    }
}

// ── SandboxConfig ──────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SandboxConfig {
    pub max_memory_pages: u32,
    pub default_fuel: u64,
    pub wasm_tools_dir: String,
}

fn default_sandbox() -> SandboxConfig {
    SandboxConfig {
        max_memory_pages: 10,
        default_fuel: 1_000_000,
        wasm_tools_dir: "tools".to_string(),
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        default_sandbox()
    }
}

impl SandboxConfig {
    pub fn validate(&self) -> Result<(), TypesError> {
        if self.max_memory_pages == 0 {
            return Err(TypesError::ConfigValidationError(
                "sandbox.max_memory_pages must be > 0".into(),
            ));
        }
        if self.default_fuel == 0 {
            return Err(TypesError::ConfigValidationError(
                "sandbox.default_fuel must be > 0".into(),
            ));
        }
        if self.wasm_tools_dir.is_empty() {
            return Err(TypesError::ConfigValidationError(
                "sandbox.wasm_tools_dir must not be empty".into(),
            ));
        }
        Ok(())
    }
}

// ── McpConfig ──────────────────────────────────────────────

/// Configuration for MCP (Model Context Protocol) tool servers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct McpConfig {
    /// Stdio-based MCP servers: prefix → "command arg1 arg2".
    #[serde(default)]
    pub stdio_servers: HashMap<String, String>,
    /// HTTP-based MCP servers: prefix → "http://host:port/path".
    #[serde(default)]
    pub http_servers: HashMap<String, String>,
}
