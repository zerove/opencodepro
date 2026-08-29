//! Integration tests for sisyphus-types `config::sub` module's
//! `ExecutorConfig` and `CacheConfig` (default + validate).

use sisyphus_types::config::sub::{CacheConfig, ExecutorConfig};

// ---------------------------------------------------------------------------
// ExecutorConfig
// ---------------------------------------------------------------------------

#[test]
fn executor_config_default() {
    let c = ExecutorConfig::default();
    assert_eq!(c.max_concurrent_tasks, 16);
    assert_eq!(c.default_fuel, 1_000_000);
}

#[test]
fn executor_config_field_access() {
    let c = ExecutorConfig {
        max_concurrent_tasks: 32,
        default_fuel: 5_000_000,
    };
    assert_eq!(c.max_concurrent_tasks, 32);
    assert_eq!(c.default_fuel, 5_000_000);
}

#[test]
fn executor_config_validate_ok() {
    let c = ExecutorConfig::default();
    assert!(c.validate().is_ok());
}

#[test]
fn executor_config_validate_zero_concurrent() {
    let c = ExecutorConfig {
        max_concurrent_tasks: 0,
        default_fuel: 1000,
    };
    assert!(c.validate().is_err());
}

#[test]
fn executor_config_validate_zero_fuel() {
    let c = ExecutorConfig {
        max_concurrent_tasks: 1,
        default_fuel: 0,
    };
    assert!(c.validate().is_err());
}

#[test]
fn executor_config_validate_error_message() {
    let c = ExecutorConfig {
        max_concurrent_tasks: 0,
        default_fuel: 1,
    };
    let err = c.validate().unwrap_err();
    let s = err.to_string();
    assert!(s.contains("max_concurrent_tasks") || s.contains("concurrent"));
}

#[test]
fn executor_config_serde() {
    let c = ExecutorConfig::default();
    let j = serde_json::to_string(&c).unwrap();
    let back: ExecutorConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.max_concurrent_tasks, c.max_concurrent_tasks);
    assert_eq!(back.default_fuel, c.default_fuel);
}

#[test]
fn executor_config_clone() {
    let a = ExecutorConfig::default();
    let b = a.clone();
    assert_eq!(a.max_concurrent_tasks, b.max_concurrent_tasks);
}

// ---------------------------------------------------------------------------
// CacheConfig
// ---------------------------------------------------------------------------

#[test]
fn cache_config_default() {
    let c = CacheConfig::default();
    assert_eq!(c.l1_capacity, 10_000);
    assert_eq!(c.database_url, "sqlite:data/sisyphus.db");
    assert!(!c.enable_vectors);
    assert_eq!(c.vector_dimension, 1536);
    assert_eq!(c.batch_size, 100);
    assert_eq!(c.flush_interval_ms, 100);
}

#[test]
fn cache_config_field_access() {
    let c = CacheConfig {
        l1_capacity: 5000,
        database_url: "postgres://localhost/x".into(),
        enable_vectors: true,
        vector_dimension: 768,
        batch_size: 50,
        flush_interval_ms: 200,
    };
    assert_eq!(c.l1_capacity, 5000);
    assert!(c.enable_vectors);
    assert_eq!(c.vector_dimension, 768);
}

#[test]
fn cache_config_validate_ok_default() {
    let c = CacheConfig::default();
    assert!(c.validate().is_ok());
}

#[test]
fn cache_config_validate_zero_l1() {
    let c = CacheConfig {
        l1_capacity: 0,
        ..CacheConfig::default()
    };
    assert!(c.validate().is_err());
}

#[test]
fn cache_config_validate_empty_db_url() {
    let c = CacheConfig {
        database_url: String::new(),
        ..CacheConfig::default()
    };
    assert!(c.validate().is_err());
}

#[test]
fn cache_config_validate_zero_vector_dim_when_enabled() {
    let c = CacheConfig {
        enable_vectors: true,
        vector_dimension: 0,
        ..CacheConfig::default()
    };
    assert!(c.validate().is_err());
}

#[test]
fn cache_config_validate_zero_vector_dim_when_disabled_ok() {
    // When enable_vectors is false, vector_dimension doesn't matter
    let c = CacheConfig {
        enable_vectors: false,
        vector_dimension: 0,
        ..CacheConfig::default()
    };
    assert!(c.validate().is_ok());
}

#[test]
fn cache_config_serde() {
    let c = CacheConfig::default();
    let j = serde_json::to_string(&c).unwrap();
    let back: CacheConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.l1_capacity, c.l1_capacity);
    assert_eq!(back.database_url, c.database_url);
}
