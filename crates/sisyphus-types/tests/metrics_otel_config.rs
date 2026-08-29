//! Integration tests for sisyphus-types `config::sub` module's
//! `MetricsConfig` and `OtelConfig` (default + validate).

use sisyphus_types::config::sub::{MetricsConfig, OtelConfig};

// ---------------------------------------------------------------------------
// MetricsConfig
// ---------------------------------------------------------------------------

#[test]
fn metrics_config_default() {
    let c = MetricsConfig::default();
    assert_eq!(c.prometheus_port, 9100);
    assert!(!c.opentelemetry.enabled);
}

#[test]
fn metrics_config_field_access() {
    let c = MetricsConfig {
        prometheus_port: 8080,
        opentelemetry: OtelConfig::default(),
    };
    assert_eq!(c.prometheus_port, 8080);
}

#[test]
fn metrics_config_validate_ok_default() {
    let c = MetricsConfig::default();
    assert!(c.validate().is_ok());
}

#[test]
fn metrics_config_validate_zero_port() {
    let c = MetricsConfig {
        prometheus_port: 0,
        opentelemetry: OtelConfig::default(),
    };
    assert!(c.validate().is_err());
}

#[test]
fn metrics_config_validate_error_message() {
    let c = MetricsConfig {
        prometheus_port: 0,
        opentelemetry: OtelConfig::default(),
    };
    let err = c.validate().unwrap_err();
    let s = err.to_string();
    assert!(s.contains("prometheus_port"));
}

#[test]
fn metrics_config_serde() {
    let c = MetricsConfig::default();
    let j = serde_json::to_string(&c).unwrap();
    let back: MetricsConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.prometheus_port, 9100);
}

// ---------------------------------------------------------------------------
// OtelConfig
// ---------------------------------------------------------------------------

#[test]
fn otel_config_default() {
    let c = OtelConfig::default();
    assert!(!c.enabled);
    assert_eq!(c.endpoint, "http://localhost:4317");
    assert_eq!(c.service_name, "sisyphus-bridge");
    assert_eq!(c.sample_rate, 1.0);
}

#[test]
fn otel_config_field_access() {
    let c = OtelConfig {
        enabled: true,
        endpoint: "https://otel.example.com:4317".into(),
        service_name: "my-service".into(),
        sample_rate: 0.5,
    };
    assert!(c.enabled);
    assert_eq!(c.service_name, "my-service");
    assert!((c.sample_rate - 0.5).abs() < 1e-9);
}

#[test]
fn otel_config_validate_ok_default() {
    let c = OtelConfig::default();
    assert!(c.validate().is_ok());
}

#[test]
fn otel_config_validate_sample_rate_negative() {
    let c = OtelConfig {
        sample_rate: -0.1,
        ..OtelConfig::default()
    };
    assert!(c.validate().is_err());
}

#[test]
fn otel_config_validate_sample_rate_above_one() {
    let c = OtelConfig {
        sample_rate: 1.1,
        ..OtelConfig::default()
    };
    assert!(c.validate().is_err());
}

#[test]
fn otel_config_validate_sample_rate_zero_ok() {
    let c = OtelConfig {
        sample_rate: 0.0,
        ..OtelConfig::default()
    };
    assert!(c.validate().is_ok());
}

#[test]
fn otel_config_validate_sample_rate_one_ok() {
    let c = OtelConfig {
        sample_rate: 1.0,
        ..OtelConfig::default()
    };
    assert!(c.validate().is_ok());
}

#[test]
fn otel_config_validate_empty_endpoint_when_enabled() {
    let c = OtelConfig {
        enabled: true,
        endpoint: String::new(),
        ..OtelConfig::default()
    };
    assert!(c.validate().is_err());
}

#[test]
fn otel_config_validate_empty_endpoint_when_disabled_ok() {
    // When disabled, empty endpoint is acceptable
    let c = OtelConfig {
        enabled: false,
        endpoint: String::new(),
        ..OtelConfig::default()
    };
    assert!(c.validate().is_ok());
}

#[test]
fn otel_config_serde() {
    let c = OtelConfig::default();
    let j = serde_json::to_string(&c).unwrap();
    let back: OtelConfig = serde_json::from_str(&j).unwrap();
    assert_eq!(back.endpoint, "http://localhost:4317");
}

#[test]
fn otel_config_clone() {
    let a = OtelConfig::default();
    let b = a.clone();
    assert_eq!(a.service_name, b.service_name);
    assert_eq!(a.sample_rate, b.sample_rate);
}
