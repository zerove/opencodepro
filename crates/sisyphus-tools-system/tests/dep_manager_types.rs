//! 集成测试：tools-system dep_manager 类型 + 同步 count_dependencies
//!
//! 覆盖 `sisyphus_tools_system::dep_manager` 中的纯数据/同步模块：
//! - `Vulnerability` 字段 + serde
//! - `OutdatedDep` 字段 + serde
//! - `DepReport` 默认 + serde
//! - `DependencyManager::count_dependencies` 基于 Cargo.toml 行扫描

use sisyphus_tools_system::dep_manager::{
    DepReport, DependencyManager, OutdatedDep, Vulnerability,
};

// ---------------------------------------------------------------------------
// Vulnerability
// ---------------------------------------------------------------------------

fn make_vuln(pkg: &str, sev: &str) -> Vulnerability {
    Vulnerability {
        package: pkg.to_string(),
        severity: sev.to_string(),
        advisory_url: format!("https://rustsec.org/advisories/{pkg}"),
        description: format!("Test vulnerability for {pkg}"),
    }
}

#[test]
fn vuln_field_access() {
    let v = make_vuln("openssl", "high");
    assert_eq!(v.package, "openssl");
    assert_eq!(v.severity, "high");
    assert!(v.advisory_url.contains("openssl"));
    assert!(v.description.contains("openssl"));
}

#[test]
fn vuln_serde_round_trip() {
    let v = make_vuln("tokio", "critical");
    let json = serde_json::to_string(&v).unwrap();
    let back: Vulnerability = serde_json::from_str(&json).unwrap();
    assert_eq!(back.package, v.package);
    assert_eq!(back.severity, v.severity);
    assert_eq!(back.advisory_url, v.advisory_url);
    assert_eq!(back.description, v.description);
}

#[test]
fn vuln_clone() {
    let v = make_vuln("x", "low");
    let v2 = v.clone();
    assert_eq!(v.package, v2.package);
}

#[test]
fn vuln_debug() {
    let v = make_vuln("x", "low");
    let s = format!("{v:?}");
    assert!(s.contains("Vulnerability"));
    assert!(s.contains("x"));
}

// ---------------------------------------------------------------------------
// OutdatedDep
// ---------------------------------------------------------------------------

fn make_outdated(name: &str, cur: &str, latest: &str) -> OutdatedDep {
    OutdatedDep {
        name: name.to_string(),
        current_version: cur.to_string(),
        latest_version: latest.to_string(),
    }
}

#[test]
fn outdated_field_access() {
    let o = make_outdated("serde", "1.0.0", "1.0.200");
    assert_eq!(o.name, "serde");
    assert_eq!(o.current_version, "1.0.0");
    assert_eq!(o.latest_version, "1.0.200");
}

#[test]
fn outdated_serde_round_trip() {
    let o = make_outdated("tokio", "1.0", "1.40");
    let json = serde_json::to_string(&o).unwrap();
    let back: OutdatedDep = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, o.name);
    assert_eq!(back.current_version, o.current_version);
    assert_eq!(back.latest_version, o.latest_version);
}

#[test]
fn outdated_clone() {
    let o = make_outdated("x", "1", "2");
    let o2 = o.clone();
    assert_eq!(o.name, o2.name);
}

// ---------------------------------------------------------------------------
// DepReport
// ---------------------------------------------------------------------------

#[test]
fn dep_report_default_empty() {
    let r = DepReport::default();
    assert!(r.vulnerabilities.is_empty());
    assert!(r.outdated.is_empty());
    assert_eq!(r.dep_count, 0);
}

#[test]
fn dep_report_with_data() {
    let r = DepReport {
        vulnerabilities: vec![make_vuln("openssl", "high")],
        outdated: vec![make_outdated("serde", "1.0.0", "1.0.200")],
        dep_count: 42,
    };
    assert_eq!(r.vulnerabilities.len(), 1);
    assert_eq!(r.outdated.len(), 1);
    assert_eq!(r.dep_count, 42);
}

#[test]
fn dep_report_serde_round_trip() {
    let r = DepReport {
        vulnerabilities: vec![make_vuln("a", "low"), make_vuln("b", "high")],
        outdated: vec![make_outdated("c", "1", "2")],
        dep_count: 10,
    };
    let json = serde_json::to_string(&r).unwrap();
    let back: DepReport = serde_json::from_str(&json).unwrap();
    assert_eq!(back.vulnerabilities.len(), 2);
    assert_eq!(back.outdated.len(), 1);
    assert_eq!(back.dep_count, 10);
}

#[test]
fn dep_report_clone() {
    let r = DepReport {
        vulnerabilities: vec![make_vuln("a", "low")],
        outdated: vec![],
        dep_count: 5,
    };
    let r2 = r.clone();
    assert_eq!(r2.vulnerabilities.len(), 1);
    assert_eq!(r2.dep_count, 5);
}

// ---------------------------------------------------------------------------
// DependencyManager::count_dependencies
// ---------------------------------------------------------------------------

#[test]
fn count_deps_empty_cargo_toml() {
    // Use a unique tempdir per test to avoid races
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sisyphus_dep_test_{}_{}",
        std::process::id(),
        nanos
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Cargo.toml");
    std::fs::write(&path, "").unwrap();

    let m = DependencyManager::new(path.to_str().unwrap());
    assert_eq!(m.count_dependencies(), 0);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn count_deps_typical_cargo_toml() {
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sisyphus_dep_test2_{}_{}",
        std::process::id(),
        nanos
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Cargo.toml");
    let toml = r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
serde = "1.0"
tokio = { version = "1.0" }
clap = "4.0"

[dev-dependencies]
proptest = "1.0"
"#;
    std::fs::write(&path, toml).unwrap();

    let m = DependencyManager::new(path.to_str().unwrap());
    let count = m.count_dependencies();
    // Should count serde, tokio, clap, proptest = 4
    assert!(count >= 4, "expected >= 4, got {count}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn count_deps_missing_file_returns_zero() {
    let m = DependencyManager::new("/nonexistent/path/to/Cargo.toml");
    assert_eq!(m.count_dependencies(), 0);
}

#[test]
fn count_deps_comments_excluded() {
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "sisyphus_dep_test3_{}_{}",
        std::process::id(),
        nanos
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Cargo.toml");
    let toml = r#"[package]
# this is a comment line
name = "test"
version = "0.1.0"

[dependencies]
# serde = "1.0"  -- commented out
serde = "1.0"
"#;
    std::fs::write(&path, toml).unwrap();

    let m = DependencyManager::new(path.to_str().unwrap());
    let count = m.count_dependencies();
    // Only `serde = "1.0"` should count; commented line excluded.
    // "name = \"test\"" and "version = \"0.1.0\"" are not deps.
    assert_eq!(count, 1);

    let _ = std::fs::remove_dir_all(&dir);
}
