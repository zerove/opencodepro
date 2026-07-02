//! DNS-over-HTTPS resolver for bypassing hijacked system DNS.
//!
//! Resolves domain names via Google/Cloudflare DoH JSON API on port 443,
//! bypassing the hijacked system DNS (port 53) in WSL environments.

use sisyphus_tools_core::{ToolError, ToolResult};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Known DoH (DNS-over-HTTPS) server IPs for bootstrapping.
const DOH_SERVERS: &[(&str, Ipv4Addr)] = &[
    ("dns.google", Ipv4Addr::new(8, 8, 8, 8)),
    ("dns.google", Ipv4Addr::new(8, 8, 4, 4)),
    ("cloudflare-dns.com", Ipv4Addr::new(1, 1, 1, 1)),
    ("cloudflare-dns.com", Ipv4Addr::new(1, 0, 0, 1)),
];

/// Resolve a domain name via Google/Cloudflare DoH JSON API on **port 443**,
/// bypassing the hijacked system DNS (port 53) in WSL environments.
async fn resolve_via_doh(domain: &str) -> ToolResult<IpAddr> {
    let mut last_err = String::from("no DoH server reachable");

    for &(doh_host, doh_ip) in DOH_SERVERS {
        let url = format!("https://{doh_host}/resolve?name={domain}&type=A");
        let addr = SocketAddr::new(IpAddr::V4(doh_ip), 443);

        let client = match reqwest::Client::builder()
            .user_agent("Sisyphus/0.1")
            .timeout(std::time::Duration::from_secs(6))
            .no_proxy()
            .resolve(doh_host, addr)
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let json = match resp.json::<serde_json::Value>().await {
                    Ok(v) => v,
                    Err(e) => {
                        last_err = e.to_string();
                        continue;
                    }
                };
                if let Some(answer) = json["Answer"].as_array() {
                    for entry in answer {
                        if entry["type"].as_i64() == Some(1) {
                            if let Some(ip_str) = entry["data"].as_str() {
                                if let Ok(ip) = ip_str.parse::<IpAddr>() {
                                    return Ok(ip);
                                }
                            }
                        }
                    }
                }
                last_err = "no A record in DoH response".into();
            }
            Ok(resp) => {
                last_err = format!("DoH status {}", resp.status());
            }
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }

    Err(ToolError::WebError(format!(
        "DNS resolution failed for {domain}: {last_err}"
    )))
}

/// Build a reqwest client with DoH-based DNS resolution and direct connection
/// (no system proxy), bypassing both DNS hijack and HTTP proxy dependency.
pub async fn build_direct_client(host: &str, port: u16) -> ToolResult<reqwest::Client> {
    let ip = resolve_via_doh(host).await?;
    let addr = SocketAddr::new(ip, port);
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
        .no_proxy()
        .resolve(host, addr)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| ToolError::WebError(format!("failed to build direct client: {e}")))
}

/// Extract hostname from a URL string.
pub fn extract_host(url: &str) -> ToolResult<&str> {
    let without = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| ToolError::WebError(format!("unsupported URL scheme: {url}")))?;
    let host = without.split('/').next().unwrap_or(without);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        return Err(ToolError::WebError(format!("empty host in URL: {url}")));
    }
    Ok(host)
}

/// Extract port from a URL string (defaults to 443 for https, 80 for http).
pub fn extract_port(url: &str) -> u16 {
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        if let Some(host_part) = rest.split('/').next() {
            if let Some(port_str) = host_part.split(':').nth(1) {
                if let Ok(port) = port_str.parse::<u16>() {
                    return port;
                }
            }
        }
    }
    if url.starts_with("https://") {
        443
    } else {
        80
    }
}
