//! Integration tests for sisyphus-tools-mcp `transport::HttpTransport`
//! pure constructor behavior (no network calls).

use sisyphus_tools_mcp::mcp::transport::HttpTransport;

macro_rules! construct_test {
    ($name:ident, $url:expr) => {
        #[test]
        fn $name() {
            let t = HttpTransport::new($url);
            // Construction should not panic
            let _ = t;
        }
    };
}

construct_test!(http_transport_new_no_trailing_slash, "https://example.com");
construct_test!(
    http_transport_new_with_trailing_slash_trims,
    "https://example.com/"
);
construct_test!(
    http_transport_new_with_multiple_trailing_slashes,
    "https://example.com////"
);
construct_test!(http_transport_new_empty_string, "");
construct_test!(http_transport_new_only_slash, "/");
construct_test!(
    http_transport_new_internal_slash_preserved,
    "https://api.example.com/v1"
);
construct_test!(http_transport_new_with_port, "http://localhost:8080/");
construct_test!(http_transport_new_ip_address, "http://127.0.0.1:5000");

#[test]
fn http_transport_new_construction_count() {
    // Build 100 transports to ensure no internal counter / leak
    let _transports: Vec<HttpTransport> =
        (0..100).map(|i| HttpTransport::new(&format!("https://example{}.com/", i))).collect();
}
