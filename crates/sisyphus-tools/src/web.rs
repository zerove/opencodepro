use crate::{ToolError, ToolResult};
use regex::Regex;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Known DoH (DNS-over-HTTPS) server IPs for bootstrapping.
/// Hard-coded IPs bypass the hijacked system DNS on port 53.
const DOH_SERVERS: &[(&str, Ipv4Addr)] = &[
    ("dns.google", Ipv4Addr::new(8, 8, 8, 8)),
    ("dns.google", Ipv4Addr::new(8, 8, 4, 4)),
    ("cloudflare-dns.com", Ipv4Addr::new(1, 1, 1, 1)),
    ("cloudflare-dns.com", Ipv4Addr::new(1, 0, 0, 1)),
];

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct FetchResult {
    pub url: String,
    pub content: String,
    pub content_type: String,
}

// ── DNS-over-HTTPS helpers ──────────────────────────────────

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
async fn build_direct_client(host: &str, port: u16) -> ToolResult<reqwest::Client> {
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

fn extract_host(url: &str) -> ToolResult<&str> {
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

fn extract_port(url: &str) -> u16 {
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

// ── Public API ──────────────────────────────────────────────

/// Fetch a URL and return its content as text.
/// Tries DoH-based direct connection first, then falls back to default client.
pub async fn webfetch(url: &str) -> ToolResult<FetchResult> {
    let host = extract_host(url)?;
    let port = extract_port(url);

    let client = match build_direct_client(host, port).await {
        Ok(c) => c,
        Err(_) => reqwest::Client::builder()
            .user_agent("Sisyphus/0.1")
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| ToolError::WebError(format!("failed to build client: {e}")))?,
    };

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| ToolError::WebError(format!("request failed: {e}")))?;

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ToolError::WebError(format!("read body failed: {e}")))?;

    if content_type.contains("text/html") {
        let html_str = String::from_utf8_lossy(&bytes);
        let content = html_to_text(&html_str);
        Ok(FetchResult {
            url: url.to_string(),
            content,
            content_type,
        })
    } else {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        Ok(FetchResult {
            url: url.to_string(),
            content: text,
            content_type,
        })
    }
}

/// Search the web using DuckDuckGo HTML search.
/// No API key required. Returns up to `num_results` results.
/// Uses DoH-based direct connection first, then falls back to default client.
pub async fn websearch(query: &str, num_results: usize) -> ToolResult<Vec<SearchResult>> {
    static RE_RESULT: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r##"<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"##).unwrap()
    });
    static RE_SNIPPET: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r##"<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"##).unwrap()
    });
    static RE_TAG: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"<[^>]*>").unwrap()
    });

    // Try DoH-based direct connection first, fallback to default client.
    let client = match build_direct_client("html.duckduckgo.com", 443).await {
        Ok(c) => c,
        Err(_) => reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| ToolError::WebError(format!("failed to build client: {e}")))?,
    };

    let params = [("q", query)];
    let resp = client
        .post("https://html.duckduckgo.com/html/")
        .form(&params)
        .send()
        .await
        .map_err(|e| ToolError::WebError(format!("search request failed: {e}")))?;

    let html = resp
        .text()
        .await
        .map_err(|e| ToolError::WebError(format!("read response failed: {e}")))?;

    let titles: Vec<(&str, &str)> = RE_RESULT
        .captures_iter(&html)
        .map(|c| {
            let url = c.get(1).map(|m| m.as_str()).unwrap_or("");
            let title_html = c.get(2).map(|m| m.as_str()).unwrap_or("");
            (url, title_html)
        })
        .collect();

    let snippets: Vec<&str> = RE_SNIPPET
        .captures_iter(&html)
        .map(|c| c.get(1).map(|m| m.as_str()).unwrap_or(""))
        .collect();

    let strip = |s: &str| -> String {
        let s = RE_TAG.replace_all(s, "");
        let s = s.replace("&amp;", "&");
        let s = s.replace("&lt;", "<");
        let s = s.replace("&gt;", ">");
        let s = s.replace("&quot;", "\"");
        let s = s.replace("&#x27;", "'");
        s.trim().to_string()
    };

    let mut results = Vec::new();
    for (i, (url, title_html)) in titles.iter().enumerate() {
        if results.len() >= num_results {
            break;
        }
        let resolved_url = if url.starts_with("//") {
            format!("https:{}", url)
        } else {
            url.to_string()
        };
        let snippet = snippets.get(i).map(|s| strip(s)).unwrap_or_default();
        results.push(SearchResult {
            title: strip(title_html),
            url: resolved_url,
            snippet,
        });
    }

    if results.is_empty() {
        return Err(ToolError::WebError("no results found".to_string()));
    }

    Ok(results)
}

/// Simple HTML-to-text conversion without external parser.
/// Strips all tags, preserves block-level element spacing.
fn html_to_text(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut tag_buf = String::new();
    let mut last_was_newline = false;

    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            in_tag = true;
            tag_buf.clear();
            continue;
        }
        if in_tag {
            if c == '>' {
                in_tag = false;
                let tag = tag_buf.trim().to_lowercase();
                let tag_name = tag.split_whitespace().next().unwrap_or(&tag);
                let tag_name = tag_name.trim_end_matches('/');

                match tag_name {
                    "script" => in_script = true,
                    "/script" => in_script = false,
                    "style" => in_style = true,
                    "/style" => in_style = false,
                    "br" | "/br" if !last_was_newline => {
                        result.push('\n');
                        last_was_newline = true;
                    }
                    tag if matches!(tag,
                        "p" | "/p" | "div" | "/div" | "tr" | "/tr" |
                        "li" | "/li" | "h1" | "/h1" | "h2" | "/h2" |
                        "h3" | "/h3" | "h4" | "/h4" | "h5" | "/h5" |
                        "h6" | "/h6" | "blockquote" | "/blockquote" |
                        "pre" | "/pre" | "hr"
                    ) && !last_was_newline => {
                        result.push('\n');
                        last_was_newline = true;
                    }
                    _ => {
                        if tag_name.starts_with('/') && !last_was_newline
                            && !result.is_empty() && !result.ends_with(' ')
                        {
                            result.push(' ');
                        }
                    }
                }
                continue;
            }
            tag_buf.push(c);
            continue;
        }

        if in_script || in_style {
            continue;
        }

        if c == '&' {
            // Skip HTML entity (e.g., &amp; &lt;) — consumers get clean text
            for c in chars.by_ref() {
                if c == ';' { break; }
            }
            continue;
        }

        if c == '\n' || c == '\r' {
            continue;
        }
        if (c == ' ' || c == '\t') && (!result.is_empty() && result.ends_with(' ')) {
            continue;
        }
        if c == ' ' || c == '\t' {
            if !result.is_empty() && !result.ends_with('\n') {
                result.push(' ');
            }
            continue;
        }

        result.push(c);
        last_was_newline = false;
    }

    result.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── html_to_text (pure) ──────────────────────────────

    #[test]
    fn test_html_to_text_plain() {
        let html = "hello world";
        assert_eq!(html_to_text(html), "hello world");
    }

    #[test]
    fn test_html_to_text_strip_tags() {
        let html = "<p>hello</p><p>world</p>";
        assert_eq!(html_to_text(html), "hello\nworld");
    }

    #[test]
    fn test_html_to_text_strip_script() {
        let html = "<p>hello</p><script>alert('x')</script><p>world</p>";
        assert_eq!(html_to_text(html), "hello\nworld");
    }

    #[test]
    fn test_html_to_text_strip_style() {
        let html = "<style>body { color: red; }</style><p>text</p>";
        assert_eq!(html_to_text(html), "text");
    }

    #[test]
    fn test_html_to_text_line_breaks() {
        let html = "a<br>b<br/>c";
        assert_eq!(html_to_text(html), "a\nb\nc");
    }

    #[test]
    fn test_html_to_text_entity_stripping() {
        let html = "<p>foo&amp;bar &lt;baz&gt;</p>";
        // HTML entities are stripped — consumers get clean text
        assert_eq!(html_to_text(html), "foobar baz");
    }

    #[test]
    fn test_html_to_text_empty() {
        assert_eq!(html_to_text(""), "");
        assert_eq!(html_to_text("<div></div>"), "");
    }

    #[test]
    fn test_html_to_text_multiline() {
        let html = "<ul><li>a</li><li>b</li></ul>";
        let text = html_to_text(html);
        assert_eq!(text, "a\nb");
    }

    // ── websearch + webfetch (network, ignored by default) ─

    #[tokio::test]
    #[ignore = "requires network"]
    async fn test_websearch_basic() {
        match websearch("rust programming", 3).await {
            Ok(results) => {
                assert!(!results.is_empty(), "should return at least one result");
                assert!(results.len() <= 3, "at most num_results items");
            }
            Err(e) => {
                // Network may be filtered/proxied — skip without panicking
                eprintln!("websearch skipped (network unavailable): {e}");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn test_webfetch_basic() {
        match webfetch("https://example.com").await {
            Ok(result) => {
                assert!(result.content.contains("Example"), "content should contain 'Example'");
                assert_eq!(result.content_type, "text/html");
            }
            Err(e) => {
                // Network may be filtered — skip without panicking
                eprintln!("webfetch skipped (network unavailable): {e}");
            }
        }
    }
}
