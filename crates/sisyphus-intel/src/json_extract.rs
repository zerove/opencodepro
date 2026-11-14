//! 从 LLM 原始响应中提取 JSON 文本。
//!
//! 本地/弱模型经常把 JSON 包在 markdown 代码栅栏（```json ... ```）里返回，
//! 而 `serde_json::from_str` 只接受纯 JSON。此前各解析点（反思、演进、
//! 合成、批判、swarm 审计）均直接 `from_str(&response)`，一旦模型加栅栏
//! 就整体失败。这里提供统一的提取函数，各解析点共用同一口径。

/// 提取响应中的 JSON 文本：
/// 1. 优先剥离 ```` ```json ... ``` ```` 栅栏；
/// 2. 其次剥离 ```` ``` ... ``` ```` 栅栏；
/// 3. 兜底返回去除首尾空白后的全文（调用方自行处理解析错误）。
pub fn extract_json(text: &str) -> String {
    // 先尝试 ```json ... ```
    if let Some(start) = text.find("```json") {
        let rest = &text[start + 7..];
        if let Some(end) = rest.find("```") {
            return rest[..end].trim().to_string();
        }
    }
    // 再尝试 ``` ... ```
    if let Some(start) = text.find("```") {
        let rest = &text[start + 3..];
        if let Some(end) = rest.find("```") {
            return rest[..end].trim().to_string();
        }
    }
    // 兜底：返回去除首尾空白后的全文
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_fenced_with_lang_tag() {
        let text = "Sure! Here is the JSON:\n```json\n{\"approved\": true}\n```\nThanks!";
        assert_eq!(extract_json(text), "{\"approved\": true}");
    }

    #[test]
    fn extracts_json_fenced_without_lang_tag() {
        let text = "```\n{\"a\": 1}\n```";
        assert_eq!(extract_json(text), "{\"a\": 1}");
    }

    #[test]
    fn passes_through_bare_json() {
        let text = "{\"a\": 1}";
        assert_eq!(extract_json(text), "{\"a\": 1}");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        let text = "  \n{\"a\": 1}\n  ";
        assert_eq!(extract_json(text), "{\"a\": 1}");
    }

    #[test]
    fn unterminated_fence_falls_back_to_full_text() {
        // 栅栏未闭合时无法可靠提取，退回全文（由调用方报错）。
        let text = "```json\n{\"a\": 1}";
        assert_eq!(extract_json(text), "```json\n{\"a\": 1}");
    }

    #[test]
    fn empty_response_stays_empty() {
        assert_eq!(extract_json(""), "");
    }
}
