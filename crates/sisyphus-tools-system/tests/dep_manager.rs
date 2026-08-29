//! Integration tests for sisyphus-tools-system `dep_manager` module.
//!
//! Exercises the pure-data types: [`Vulnerability`], [`OutdatedDep`],
//! and [`DepReport`]. The actual `DependencyManager::new` only stores
//! a path, so we don't exercise it here (would require a real manifest).

use sisyphus_tools_system::dep_manager::{DepReport, OutdatedDep, Vulnerability};

// ---------------------------------------------------------------------------
// Vulnerability
// ---------------------------------------------------------------------------

#[test]
fn vulnerability_field_assignment() {
    let v = Vulnerability {
        package: "openssl".into(),
        severity: "high".into(),
        advisory_url: "https://rustsec.org/advisories/123".into(),
        description: "use-after-free".into(),
    };
    assert_eq!(v.package, "openssl");
    assert_eq!(v.severity, "high");
}

#[test]
fn vulnerability_serde() {
    let v = Vulnerability {
        package: "p".into(),
        severity: "low".into(),
        advisory_url: "x".into(),
        description: "d".into(),
    };
    let j = serde_json::to_string(&v).unwrap();
    let back: Vulnerability = serde_json::from_str(&j).unwrap();
    assert_eq!(back.package, "p");
}

// ---------------------------------------------------------------------------
// OutdatedDep
// ---------------------------------------------------------------------------

#[test]
fn outdated_dep_field_assignment() {
    let o = OutdatedDep {
        name: "serde".into(),
        current_version: "1.0.100".into(),
        latest_version: "1.0.200".into(),
    };
    assert_eq!(o.name, "serde");
    assert_eq!(o.current_version, "1.0.100");
}

#[test]
fn outdated_dep_serde() {
    let o = OutdatedDep {
        name: "n".into(),
        current_version: "1.0.0".into(),
        latest_version: "2.0.0".into(),
    };
    let j = serde_json::to_string(&o).unwrap();
    let back: OutdatedDep = serde_json::from_str(&j).unwrap();
    assert_eq!(back.latest_version, "2.0.0");
}

// ---------------------------------------------------------------------------
// DepReport
// ---------------------------------------------------------------------------

#[test]
fn dep_report_default_is_empty() {
    let r = DepReport::default();
    assert!(r.vulnerabilities.is_empty());
    assert!(r.outdated.is_empty());
    assert_eq!(r.dep_count, 0);
}

#[test]
fn dep_report_with_data() {
    let r = DepReport {
        vulnerabilities: vec![Vulnerability {
            package: "x".into(),
            severity: "high".into(),
            advisory_url: "u".into(),
            description: "d".into(),
        }],
        outdated: vec![OutdatedDep {
            name: "y".into(),
            current_version: "1".into(),
            latest_version: "2".into(),
        }],
        dep_count: 50,
    };
    assert_eq!(r.vulnerabilities.len(), 1);
    assert_eq!(r.outdated.len(), 1);
    assert_eq!(r.dep_count, 50);
}

#[test]
fn dep_report_serde() {
    let r = DepReport {
        vulnerabilities: vec![],
        outdated: vec![],
        dep_count: 10,
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: DepReport = serde_json::from_str(&j).unwrap();
    assert_eq!(back.dep_count, 10);
}
