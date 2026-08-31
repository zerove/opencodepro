//! 集成测试：tools-system web::resolver URL 解析
//!
//! 覆盖 `sisyphus_tools_system::web::resolver`：
//! - `extract_host` 解析多种 URL 格式
//! - `extract_port` 显式/默认

use sisyphus_tools_system::web::resolver::{extract_host, extract_port};

// ---------------------------------------------------------------------------
// extract_host
// ---------------------------------------------------------------------------

#[test]
fn host_https_simple() {
    assert_eq!(extract_host("https://example.com").unwrap(), "example.com");
}

#[test]
fn host_https_with_path() {
    assert_eq!(
        extract_host("https://example.com/path/to/file").unwrap(),
        "example.com"
    );
}

#[test]
fn host_https_with_port() {
    assert_eq!(
        extract_host("https://example.com:8080").unwrap(),
        "example.com"
    );
}

#[test]
fn host_https_with_port_and_path() {
    assert_eq!(
        extract_host("https://example.com:8080/api/v1/users").unwrap(),
        "example.com"
    );
}

#[test]
fn host_http_simple() {
    assert_eq!(extract_host("http://example.com").unwrap(), "example.com");
}

#[test]
fn host_http_with_port() {
    assert_eq!(extract_host("http://localhost:3000").unwrap(), "localhost");
}

#[test]
fn host_subdomain() {
    assert_eq!(
        extract_host("https://api.example.com").unwrap(),
        "api.example.com"
    );
}

#[test]
fn host_ip_address() {
    assert_eq!(extract_host("https://127.0.0.1:8080").unwrap(), "127.0.0.1");
}

#[test]
fn host_unsupported_scheme() {
    assert!(extract_host("ftp://example.com").is_err());
    assert!(extract_host("file:///tmp/file").is_err());
    assert!(extract_host("ws://example.com").is_err());
}

#[test]
fn host_empty_after_scheme() {
    assert!(extract_host("https://").is_err());
}

#[test]
fn host_no_scheme() {
    assert!(extract_host("example.com").is_err());
}

// ---------------------------------------------------------------------------
// extract_port
// ---------------------------------------------------------------------------

#[test]
fn port_https_default() {
    assert_eq!(extract_port("https://example.com"), 443);
}

#[test]
fn port_http_default() {
    assert_eq!(extract_port("http://example.com"), 80);
}

#[test]
fn port_explicit_https() {
    assert_eq!(extract_port("https://example.com:8443"), 8443);
}

#[test]
fn port_explicit_http() {
    assert_eq!(extract_port("http://example.com:8080"), 8080);
}

#[test]
fn port_explicit_with_path() {
    assert_eq!(extract_port("https://example.com:9443/api"), 9443);
}

#[test]
fn port_invalid_uses_default() {
    // Malformed port falls back to scheme default
    assert_eq!(extract_port("https://example.com:abc"), 443);
}

#[test]
fn port_zero_explicit() {
    assert_eq!(extract_port("http://example.com:0"), 0);
}

#[test]
fn port_max_explicit() {
    assert_eq!(extract_port("https://example.com:65535"), 65535);
}

#[test]
fn port_overflow_uses_default() {
    // 65536 overflows u16; falls back to scheme default
    assert_eq!(extract_port("https://example.com:65536"), 443);
}

#[test]
fn port_no_scheme_defaults_80() {
    // No recognized scheme → 80 default
    assert_eq!(extract_port("example.com"), 80);
}

#[test]
fn port_localhost_with_port() {
    assert_eq!(extract_port("http://localhost:3000"), 3000);
}
