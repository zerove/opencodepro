//! Web fetching and searching.
//!
//! Provides `webfetch` for URL content retrieval and `websearch` for
//! DuckDuckGo HTML search, with DoH-based DNS bypass for WSL environments.
//!
//! ## Sub-modules
//! - [`types`]: Shared types (SearchResult, FetchResult).
//! - [`resolver`]: DNS-over-HTTPS resolution and URL parsing.

pub mod resolver;
pub mod types;

pub use types::{FetchResult, SearchResult};

use regex::Regex;
pub use resolver::{build_direct_client, extract_host, extract_port};
use sisyphus_tools_core::{ToolError, ToolResult};

/// 编译 DuckDuckGo 正则，错误传播为 `ToolError`。
/// 正则为编译期字面量，失败极不可能；不缓存避免生命周期问题。
fn ddg_re(pattern: &str) -> ToolResult<Regex> {
    Regex::new(pattern)
        .map_err(|e| ToolError::WebError(format!("regex '{pattern}' compile failed: {e}")))
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

    // 必须检查状态码：404/500 的错误页面同样是 HTML，若直接返回会被
    // 当作正常内容交给调用方（表现为「抓到了内容但内容是错误页」）。
    let status = resp.status();
    if !status.is_success() {
        return Err(ToolError::WebError(format!(
            "fetch {url} returned HTTP {status}"
        )));
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let bytes =
        resp.bytes().await.map_err(|e| ToolError::WebError(format!("read body failed: {e}")))?;

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
    websearch_via(
        "https://html.duckduckgo.com/html/",
        "html.duckduckgo.com",
        443,
        query,
        num_results,
    )
    .await
}

/// websearch 的可注入端点版本
///
/// 端点/主机/端口分离传入，使状态码处理与结果解析等逻辑可在本地回环服务器上
/// 被测试覆盖（真实 DuckDuckGo 会返回 202 反爬页，无法稳定用于断言）。
async fn websearch_via(
    endpoint: &str,
    host: &str,
    port: u16,
    query: &str,
    num_results: usize,
) -> ToolResult<Vec<SearchResult>> {
    let re_result = ddg_re(r#"<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#)?;
    let re_snippet = ddg_re(r#"<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#)?;
    let re_tag = ddg_re(r"<[^>]*>")?;

    // Try DoH-based direct connection first, fallback to default client.
    let client = match build_direct_client(host, port).await {
        Ok(c) => c,
        Err(_) => reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| ToolError::WebError(format!("failed to build client: {e}")))?,
    };

    let params = [("q", query)];
    let resp = client
        .post(endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| ToolError::WebError(format!("search request failed: {e}")))?;

    // 必须检查状态码：DuckDuckGo 对疑似自动化请求返回 202 并附反爬页面，
    // 该页面能正常「解析」但不含任何结果。不检查就会把上游拒绝误报成
    // 「no results found」，调用方无法区分「确实无结果」与「被限流/拦截」。
    //
    // 注意必须是精确 200 而不是 is_success()：DuckDuckGo 反爬返回 202，
    // 而 202 属于 2xx，is_success() 会放行，反爬页照样被误报成零结果。
    let status = resp.status();
    if status != reqwest::StatusCode::OK {
        return Err(ToolError::WebError(format!(
            "search endpoint returned HTTP {status} (likely rate-limited or blocked)"
        )));
    }

    let html =
        resp.text().await.map_err(|e| ToolError::WebError(format!("read response failed: {e}")))?;

    let titles: Vec<(&str, &str)> = re_result
        .captures_iter(&html)
        .map(|c| {
            let url = c.get(1).map_or("", |m| m.as_str());
            let title_html = c.get(2).map_or("", |m| m.as_str());
            (url, title_html)
        })
        .collect();

    let snippets: Vec<&str> =
        re_snippet.captures_iter(&html).map(|c| c.get(1).map_or("", |m| m.as_str())).collect();

    let strip = |s: &str| -> String {
        let s = re_tag.replace_all(s, "");
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
            format!("https:{url}")
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
                    },
                    tag if matches!(
                        tag,
                        "p" | "/p"
                            | "div"
                            | "/div"
                            | "tr"
                            | "/tr"
                            | "li"
                            | "/li"
                            | "h1"
                            | "/h1"
                            | "h2"
                            | "/h2"
                            | "h3"
                            | "/h3"
                            | "h4"
                            | "/h4"
                            | "h5"
                            | "/h5"
                            | "h6"
                            | "/h6"
                            | "blockquote"
                            | "/blockquote"
                            | "pre"
                            | "/pre"
                            | "hr"
                    ) && !last_was_newline =>
                    {
                        result.push('\n');
                        last_was_newline = true;
                    },
                    _ => {
                        if tag_name.starts_with('/')
                            && !last_was_newline
                            && !result.is_empty()
                            && !result.ends_with(' ')
                        {
                            result.push(' ');
                        }
                    },
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
                if c == ';' {
                    break;
                }
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

    result.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
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

    #[test]
    fn test_html_to_text_nested_tags() {
        let html = "<div><p>hello <b>world</b></p></div>";
        assert_eq!(html_to_text(html), "hello world");
    }

    #[test]
    fn test_html_to_text_extra_whitespace_collapsed() {
        let html = "<p>  spaces   everywhere  </p>";
        assert_eq!(html_to_text(html), "spaces everywhere");
    }

    #[test]
    fn test_html_to_text_mixed_block_inline() {
        let html = "<h1>Title</h1><p>Para <a>link</a></p>";
        let text = html_to_text(html);
        assert_eq!(text, "Title\nPara link");
    }

    #[test]
    fn test_html_to_text_self_closing_tags() {
        let html = "<hr/><p>after</p>";
        assert_eq!(html_to_text(html), "after");
    }

    #[test]
    fn test_html_to_text_newlines_in_html() {
        let html = "<p>line1\nline2</p>";
        assert_eq!(html_to_text(html), "line1line2");
    }

    #[test]
    fn test_html_to_text_paragraphs_with_text() {
        let html = "<p>first</p>\n<p>second</p>\n<p>third</p>";
        assert_eq!(html_to_text(html), "first\nsecond\nthird");
    }

    // ── extract_host (pure) ──────────────────────────────

    #[test]
    fn test_extract_host_https() {
        assert_eq!(
            extract_host("https://example.com/path").unwrap(),
            "example.com"
        );
    }

    #[test]
    fn test_extract_host_with_port() {
        assert_eq!(
            extract_host("https://example.com:8080/path").unwrap(),
            "example.com"
        );
    }

    #[test]
    fn test_extract_host_http() {
        assert_eq!(
            extract_host("http://sub.example.org/page").unwrap(),
            "sub.example.org"
        );
    }

    #[test]
    fn test_extract_host_no_path() {
        assert_eq!(extract_host("https://example.com").unwrap(), "example.com");
    }

    #[test]
    fn test_extract_host_invalid_scheme() {
        let result = extract_host("ftp://example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_host_empty_string() {
        let result = extract_host("");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_host_empty_host() {
        let result = extract_host("https:///path");
        assert!(result.is_err());
    }

    // ── extract_port (pure) ──────────────────────────────

    #[test]
    fn test_extract_port_default_https() {
        assert_eq!(extract_port("https://example.com"), 443);
    }

    #[test]
    fn test_extract_port_default_http() {
        assert_eq!(extract_port("http://example.com"), 80);
    }

    #[test]
    fn test_extract_port_custom() {
        assert_eq!(extract_port("https://example.com:3000"), 3000);
    }

    #[test]
    fn test_extract_port_custom_http() {
        assert_eq!(extract_port("http://example.com:8080"), 8080);
    }

    #[test]
    fn test_extract_port_with_path() {
        assert_eq!(extract_port("https://example.com:9090/api/v1"), 9090);
    }

    #[test]
    fn test_extract_port_missing_scheme_defaults_http() {
        assert_eq!(extract_port("example.com"), 80);
    }

    // ── websearch + webfetch (network, ignored by default) ─

    /// 网络不可用时**必须失败**，不得静默跳过。
    ///
    /// 原写法用 `match { Ok(..) => assert, Err(e) => eprintln!(..) }`，
    /// 于是断网/DNS 失败时该用例照样报 `passed` —— 与本仓库已修掉的
    /// 6 个「守卫失效」缺陷同类（全通过却报 passed=0、
    /// 部分构建失败漏判等）。改为 `expect`，让「跳过」变成显式失败，
    /// 真正跑这个用例（`cargo test -- --ignored`）时才有判别力。
    #[tokio::test]
    #[ignore = "requires network"]
    async fn test_websearch_basic() {
        let results = websearch("rust programming", 3)
            .await
            .expect("websearch 不应失败：本用例已显式 #[ignore]，手动运行时必须有真实网络");
        assert!(!results.is_empty(), "should return at least one result");
        assert!(results.len() <= 3, "at most num_results items");
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn test_webfetch_basic() {
        let result = webfetch("https://example.com")
            .await
            .expect("webfetch 不应失败：本用例已显式 #[ignore]，手动运行时必须有真实网络");
        assert!(
            result.content.contains("Example"),
            "content should contain 'Example'"
        );
        assert!(
            result.content_type.contains("text/html"),
            "content_type should be HTML, got {:?}",
            result.content_type
        );
    }

    // ── HTTP 错误处理（本地回环，不依赖外网） ─

    /// 起一个只回固定状态行的本地 HTTP 服务器，返回其端口。
    async fn spawn_status_server(status_line: &'static str) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let body = "<html><body>error page</body></html>";
                let resp = format!(
                    "HTTP/1.1 {status_line}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.read(&mut [0u8; 1024]).await;
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        port
    }

    #[tokio::test]
    async fn test_webfetch_rejects_error_status_instead_of_returning_error_page() {
        // 404 的响应体是 HTML 错误页：若不检查状态码，会被当作正常内容返回
        let port = spawn_status_server("404 Not Found").await;
        let url = format!("http://127.0.0.1:{port}/missing");
        let err = webfetch(&url).await.expect_err("404 必须报错而非返回错误页内容");
        let msg = err.to_string();
        assert!(msg.contains("404"), "错误信息应含状态码，实际: {msg}");
    }

    #[tokio::test]
    async fn test_webfetch_accepts_ok_status() {
        let port = spawn_status_server("200 OK").await;
        let url = format!("http://127.0.0.1:{port}/ok");
        let result = webfetch(&url).await.expect("200 应正常返回");
        assert!(
            result.content.contains("error page"),
            "200 响应体应被正常提取，实际: {}",
            result.content
        );
    }

    #[tokio::test]
    async fn test_webfetch_rejects_server_error() {
        let port = spawn_status_server("500 Internal Server Error").await;
        let url = format!("http://127.0.0.1:{port}/boom");
        let err = webfetch(&url).await.expect_err("500 必须报错");
        assert!(err.to_string().contains("500"), "实际: {err}");
    }

    #[tokio::test]
    async fn test_websearch_reports_block_status_instead_of_no_results() {
        // 202 是 DuckDuckGo 的反爬响应：响应体是能解析但无结果的 HTML。
        // 若不检查状态码，调用方只会看到「no results found」，无从判断
        // 是「确实没搜到」还是「被上游拦截」。
        let port = spawn_status_server("202 Accepted").await;
        let endpoint = format!("http://127.0.0.1:{port}/html");
        let err = websearch_via(&endpoint, "127.0.0.1", port, "rust", 3)
            .await
            .expect_err("202 必须报错而非返回零结果");
        let msg = err.to_string();
        assert!(msg.contains("202"), "错误信息应含状态码，实际: {msg}");
    }

    #[tokio::test]
    async fn test_websearch_parses_results_from_ok_response() {
        let port = spawn_ddg_result_server().await;
        let endpoint = format!("http://127.0.0.1:{port}/html");
        let results = websearch_via(&endpoint, "127.0.0.1", port, "rust", 3)
            .await
            .expect("200 且含结果时应正常解析");
        assert_eq!(results.len(), 1, "应解析出 1 条结果");
        assert_eq!(results[0].title, "Rust Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
    }

    /// 返回一段符合 DuckDuckGo 结果结构的 HTML，验证解析路径未被状态码检查误伤
    async fn spawn_ddg_result_server() -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let body = concat!(
                    r#"<div class="result"><a class="result__a" href="https://www.rust-lang.org/">"#,
                    "Rust Language</a>",
                    r#"<a class="result__snippet">A language empowering everyone</a></div>"#
                );
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.read(&mut [0u8; 1024]).await;
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.flush().await;
            }
        });
        port
    }
}
