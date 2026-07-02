use serde_json::{json, Value};

// ── argument parsing helpers ─────────────────────────────────

pub fn parse_args(args: &[u8]) -> Result<Value, String> {
    if args.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(args).map_err(|e| format!("invalid JSON args: {e}"))
}

pub fn str_arg(v: &Value, name: &str) -> Result<String, String> {
    v.get(name)
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
        .ok_or_else(|| format!("missing or non-string arg '{name}'"))
}

pub fn path_arg(v: &Value, name: &str) -> Result<String, String> {
    v.get(name)
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
        .ok_or_else(|| format!("missing or non-string arg '{name}'"))
}

pub fn flexible_str_arg(v: &Value, primary: &str, fallback: &str) -> Result<String, String> {
    flexible_str_arg_multi(v, &[primary, fallback])
}

pub fn flexible_str_arg_multi(v: &Value, keys: &[&str]) -> Result<String, String> {
    for key in keys {
        if let Some(val) = v.get(*key).and_then(|v| v.as_str()) {
            return Ok(val.to_string());
        }
    }
    let keys_str: Vec<_> = keys.iter().map(|k| (*k).to_string()).collect();
    Err(format!(
        "missing or non-string arg in set: {}",
        keys_str.join(", ")
    ))
}

pub fn u64_arg(v: &Value, name: &str) -> Result<u64, String> {
    v.get(name)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("missing or non-integer arg '{name}'"))
}

pub fn usize_arg(v: &Value, name: &str) -> Result<usize, String> {
    u64_arg(v, name).map(|n| n as usize)
}

pub fn opt_str_arg(v: &Value, name: &str) -> Option<String> {
    v.get(name).and_then(|v| v.as_str()).map(std::string::ToString::to_string)
}

pub fn ok_json(val: Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&val).map_err(|e| format!("serialize result: {e}"))
}
