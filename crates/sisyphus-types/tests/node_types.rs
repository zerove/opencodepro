//! Integration tests for sisyphus-types `node` module's pure data
//! types. Covers [`ExecutionMeasurement`], [`ReflectionPriority`],
//! [`TestResult`], and [`ToolMetadata`].

use sisyphus_types::node::{ExecutionMeasurement, ReflectionPriority, TestResult, ToolMetadata};

// ---------------------------------------------------------------------------
// ExecutionMeasurement
// ---------------------------------------------------------------------------

#[test]
fn execution_measurement_field_assignment() {
    let m = ExecutionMeasurement {
        fuel_consumed: 100,
        duration_ms: 50,
        peak_memory_bytes: Some(4096),
    };
    assert_eq!(m.fuel_consumed, 100);
    assert_eq!(m.duration_ms, 50);
    assert_eq!(m.peak_memory_bytes, Some(4096));
}

#[test]
fn execution_measurement_serde() {
    let m = ExecutionMeasurement {
        fuel_consumed: 0,
        duration_ms: 0,
        peak_memory_bytes: None,
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: ExecutionMeasurement = serde_json::from_str(&j).unwrap();
    assert_eq!(back, m);
}

/// `peak_memory_bytes` was added after the first release, so payloads already
/// on disk (and older daemons) lack the key entirely. Deserialising must yield
/// `None`, not fail — otherwise every historical event record breaks.
#[test]
fn execution_measurement_deserializes_without_peak_memory() {
    let back: ExecutionMeasurement =
        serde_json::from_str(r#"{"fuel_consumed":7,"duration_ms":9}"#).unwrap();
    assert_eq!(back.fuel_consumed, 7);
    assert_eq!(back.peak_memory_bytes, None);
}

/// `None` must stay absent on the wire rather than serialising as `null`:
/// omitting is what keeps old daemons and new clients interoperable.
#[test]
fn execution_measurement_omits_absent_peak_memory() {
    let m = ExecutionMeasurement {
        fuel_consumed: 1,
        duration_ms: 2,
        peak_memory_bytes: None,
    };
    let j = serde_json::to_string(&m).unwrap();
    assert!(!j.contains("peak_memory_bytes"), "unexpected key in {j}");
}

#[test]
fn execution_measurement_equality() {
    let m1 = ExecutionMeasurement {
        fuel_consumed: 1,
        duration_ms: 2,
        peak_memory_bytes: Some(64),
    };
    let m2 = ExecutionMeasurement {
        fuel_consumed: 1,
        duration_ms: 2,
        peak_memory_bytes: Some(64),
    };
    assert_eq!(m1, m2);
}

#[test]
fn execution_measurement_inequality() {
    let m1 = ExecutionMeasurement {
        fuel_consumed: 1,
        duration_ms: 2,
        peak_memory_bytes: None,
    };
    let m2 = ExecutionMeasurement {
        fuel_consumed: 2,
        duration_ms: 2,
        peak_memory_bytes: None,
    };
    assert_ne!(m1, m2);
}

/// Same fuel and duration but different memory must NOT compare equal —
/// otherwise the new dimension would be invisible to anomaly detection, which
/// is the whole reason this field exists.
#[test]
fn execution_measurement_inequality_on_peak_memory() {
    let m1 = ExecutionMeasurement {
        fuel_consumed: 1,
        duration_ms: 2,
        peak_memory_bytes: Some(1024),
    };
    let m2 = ExecutionMeasurement {
        fuel_consumed: 1,
        duration_ms: 2,
        peak_memory_bytes: Some(4096),
    };
    assert_ne!(m1, m2);
}

// ---------------------------------------------------------------------------
// ReflectionPriority
// ---------------------------------------------------------------------------

#[test]
fn reflection_priority_variants_distinct() {
    let variants = [
        ReflectionPriority::None,
        ReflectionPriority::Low,
        ReflectionPriority::High,
    ];
    let mut seen = std::collections::HashSet::new();
    for v in variants {
        let d = format!("{v:?}");
        assert!(seen.insert(d.clone()));
    }
}

#[test]
fn reflection_priority_equality() {
    assert_eq!(ReflectionPriority::None, ReflectionPriority::None);
    assert_ne!(ReflectionPriority::None, ReflectionPriority::High);
}

// ---------------------------------------------------------------------------
// TestResult
// ---------------------------------------------------------------------------

#[test]
fn test_result_field_assignment() {
    let r = TestResult {
        success: true,
        stdout: "out".into(),
        stderr: "err".into(),
        exit_code: 0,
        duration_ms: 100,
        error_summary: None,
    };
    assert!(r.success);
    assert_eq!(r.exit_code, 0);
    assert!(r.error_summary.is_none());
}

#[test]
fn test_result_serde() {
    let r = TestResult {
        success: false,
        stdout: "".into(),
        stderr: "boom".into(),
        exit_code: 1,
        duration_ms: 50,
        error_summary: Some("test failed".into()),
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: TestResult = serde_json::from_str(&j).unwrap();
    assert!(!back.success);
    assert_eq!(back.error_summary.as_deref(), Some("test failed"));
}

// ---------------------------------------------------------------------------
// ToolMetadata
// ---------------------------------------------------------------------------

#[test]
fn tool_metadata_field_assignment() {
    let m = ToolMetadata {
        id: "read".into(),
        description: "read a file".into(),
    };
    assert_eq!(m.id, "read");
    assert_eq!(m.description, "read a file");
}

#[test]
fn tool_metadata_serde() {
    let m = ToolMetadata {
        id: "t".into(),
        description: "d".into(),
    };
    let j = serde_json::to_string(&m).unwrap();
    let back: ToolMetadata = serde_json::from_str(&j).unwrap();
    assert_eq!(back.id, "t");
}
