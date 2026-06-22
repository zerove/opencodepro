use std::collections::HashMap;
use crate::{ToolError, ToolResult};

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

/// Fetch a URL and return its content as text.
pub async fn webfetch(url: &str) -> ToolResult<FetchResult> {
    let client = reqwest::Client::builder()
        .user_agent("Sisyphus/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| ToolError::WebError(format!("failed to build client: {e}")))?;

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

/// Search the web using DuckDuckGo Instant Answer API.
/// No API key required. Returns up to `num_results` results.
pub async fn websearch(query: &str, num_results: usize) -> ToolResult<Vec<SearchResult>> {
    let client = reqwest::Client::builder()
        .user_agent("Sisyphus/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| ToolError::WebError(format!("failed to build client: {e}")))?;

    let params = HashMap::from([
        ("q", query),
        ("format", "json"),
        ("no_html", "1"),
        ("skip_disambig", "1"),
    ]);

    let resp = client
        .get("https://api.duckduckgo.com/")
        .query(&params)
        .send()
        .await
        .map_err(|e| ToolError::WebError(format!("search request failed: {e}")))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ToolError::WebError(format!("parse search response failed: {e}")))?;

    let mut results = Vec::new();

    if let Some(answer) = body.get("AbstractText").and_then(|v| v.as_str()) {
        if !answer.is_empty() {
            let title = body
                .get("AbstractSource")
                .and_then(|v| v.as_str())
                .unwrap_or("DuckDuckGo");
            let url = body
                .get("AbstractURL")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            results.push(SearchResult {
                title: title.to_string(),
                url: url.to_string(),
                snippet: answer.to_string(),
            });
        }
    }

    if let Some(topics) = body.get("RelatedTopics").and_then(|v| v.as_array()) {
        for topic in topics {
            if results.len() >= num_results {
                break;
            }
            if let Some(text) = topic.get("Text").and_then(|v| v.as_str()) {
                let url = topic
                    .get("FirstURL")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                results.push(SearchResult {
                    title: text.split(" - ").next().unwrap_or(text).to_string(),
                    url: url.to_string(),
                    snippet: text.to_string(),
                });
            }
        }
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

    // ── websearch (network, ignored by default) ──────────

    #[tokio::test]
    #[ignore = "requires network"]
    async fn test_websearch_basic() {
        let results = websearch("rust programming", 3).await.unwrap();
        assert!(!results.is_empty());
        assert!(results.len() <= 3);
    }

    #[tokio::test]
    #[ignore = "requires network"]
    async fn test_webfetch_basic() {
        let result = webfetch("https://example.com").await.unwrap();
        assert!(result.content.contains("Example"));
        assert_eq!(result.content_type, "text/html");
    }
}
