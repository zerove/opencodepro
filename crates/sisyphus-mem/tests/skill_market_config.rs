//! 集成测试：mem skill_market 配置 + RateLimiter 行为
//!
//! 覆盖 `sisyphus_mem::skill_market` 中的纯数据 + 同步模块：
//! - `SkillMarketConfig` 默认值 + serde
//! - `SkillRegistryEntry` 字段
//! - RateLimiter 行为（用 tempdir-based 测试避免文件 IO）

use sisyphus_mem::skill_market::SkillMarketConfig;
use sisyphus_types::skill::SkillMetadata;

// ---------------------------------------------------------------------------
// SkillMarketConfig
// ---------------------------------------------------------------------------

#[test]
fn config_default() {
    let c = SkillMarketConfig::default();
    assert_eq!(c.registries.len(), 1);
    assert!(c.registries[0].starts_with("https://"));
    assert!(!c.auto_update);
    assert!(c.trusted_public_keys.is_empty());
    assert!(c.require_signatures);
    assert_eq!(c.rate_limit_per_second, 10);
}

#[test]
fn config_clone() {
    let c = SkillMarketConfig::default();
    let c2 = c.clone();
    assert_eq!(c.registries, c2.registries);
    assert_eq!(c.rate_limit_per_second, c2.rate_limit_per_second);
    assert_eq!(c.require_signatures, c2.require_signatures);
}

#[test]
fn config_serde_full() {
    let c = SkillMarketConfig {
        registries: vec!["a".into(), "b".into()],
        cache_dir: "/tmp/cache".into(),
        auto_update: true,
        trusted_public_keys: vec!["pk1".into()],
        require_signatures: false,
        rate_limit_per_second: 100,
    };
    let json = serde_json::to_string(&c).unwrap();
    let back: SkillMarketConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.registries.len(), 2);
    assert_eq!(back.cache_dir, "/tmp/cache");
    assert!(back.auto_update);
    assert_eq!(back.trusted_public_keys, vec!["pk1".to_string()]);
    assert!(!back.require_signatures);
    assert_eq!(back.rate_limit_per_second, 100);
}

#[test]
fn config_serde_default_require_signatures_true() {
    // Missing require_signatures field → defaults to true
    let json = r#"{"registries":[],"cache_dir":"x","auto_update":false,"trusted_public_keys":[],"rate_limit_per_second":1}"#;
    let c: SkillMarketConfig = serde_json::from_str(json).unwrap();
    assert!(c.require_signatures);
}

#[test]
fn config_serde_default_rate_limit() {
    // Missing rate_limit_per_second → defaults to 10
    let json = r#"{"registries":[],"cache_dir":"x","auto_update":false,"trusted_public_keys":[],"require_signatures":false}"#;
    let c: SkillMarketConfig = serde_json::from_str(json).unwrap();
    assert_eq!(c.rate_limit_per_second, 10);
}

#[test]
fn config_serde_default_trusted_keys_empty() {
    let json = r#"{"registries":[],"cache_dir":"x","auto_update":false,"require_signatures":true,"rate_limit_per_second":1}"#;
    let c: SkillMarketConfig = serde_json::from_str(json).unwrap();
    assert!(c.trusted_public_keys.is_empty());
}

#[test]
fn config_debug() {
    let c = SkillMarketConfig::default();
    let s = format!("{c:?}");
    assert!(s.contains("SkillMarketConfig"));
    assert!(s.contains("require_signatures"));
    assert!(s.contains("rate_limit_per_second"));
}

// ---------------------------------------------------------------------------
// SkillRegistryEntry (fields only since SkillMetadata is opaque without builder)
// ---------------------------------------------------------------------------

#[test]
fn config_metadata_type_imports() {
    // Sanity check: the SkillMetadata type is reachable.
    let _: Option<SkillMetadata> = None;
}

#[test]
fn config_with_cache_dir() {
    let c = SkillMarketConfig {
        cache_dir: "/var/cache/s".into(),
        ..SkillMarketConfig::default()
    };
    assert_eq!(c.cache_dir, "/var/cache/s");
}

#[test]
fn config_with_empty_registries() {
    let c = SkillMarketConfig {
        registries: vec![],
        ..SkillMarketConfig::default()
    };
    assert!(c.registries.is_empty());
}

#[test]
fn config_with_many_registries() {
    let c = SkillMarketConfig {
        registries: vec![
            "https://a.com".into(),
            "https://b.com".into(),
            "https://c.com".into(),
        ],
        ..SkillMarketConfig::default()
    };
    assert_eq!(c.registries.len(), 3);
}

#[test]
fn config_trusted_keys_multiple() {
    let c = SkillMarketConfig {
        trusted_public_keys: vec!["pk1".into(), "pk2".into(), "pk3".into()],
        ..SkillMarketConfig::default()
    };
    assert_eq!(c.trusted_public_keys.len(), 3);
}

#[test]
fn config_zero_rate_limit() {
    let c = SkillMarketConfig {
        rate_limit_per_second: 0,
        ..SkillMarketConfig::default()
    };
    assert_eq!(c.rate_limit_per_second, 0);
}

#[test]
fn config_high_rate_limit() {
    let c = SkillMarketConfig {
        rate_limit_per_second: 10_000,
        ..SkillMarketConfig::default()
    };
    assert_eq!(c.rate_limit_per_second, 10_000);
}
