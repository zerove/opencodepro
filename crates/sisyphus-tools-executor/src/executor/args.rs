use serde_json::{json, Value};

// ── argument parsing helpers ─────────────────────────────────

/// Parse JSON argument bytes into a `serde_json::Value`.
///
/// Returns an empty object `{}` if `args` is empty (commands that take no
/// arguments are valid).
///
/// # Errors
/// Returns an error string if the bytes are not valid JSON.
pub fn parse_args(args: &[u8]) -> Result<Value, String> {
    if args.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(args).map_err(|e| format!("invalid JSON args: {e}"))
}

/// Extract a required string argument by name from a JSON object.
///
/// # Errors
/// Returns an error string if `name` is missing or not a JSON string.
pub fn str_arg(v: &Value, name: &str) -> Result<String, String> {
    v.get(name)
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
        .ok_or_else(|| format!("missing or non-string arg '{name}'"))
}

/// Extract a required path argument by name (semantically identical to [`str_arg`]
/// but kept distinct for readability at call sites).
pub fn path_arg(v: &Value, name: &str) -> Result<String, String> {
    v.get(name)
        .and_then(|v| v.as_str())
        .map(std::string::ToString::to_string)
        .ok_or_else(|| format!("missing or non-string arg '{name}'"))
}

/// Try the `primary` key first, fall back to `fallback`. See [`flexible_str_arg_multi`].
pub fn flexible_str_arg(v: &Value, primary: &str, fallback: &str) -> Result<String, String> {
    flexible_str_arg_multi(v, &[primary, fallback])
}

/// Try each key in `keys` in order; return the first one that exists as a string.
///
/// # Errors
/// Returns an error listing all tried keys if none match.
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

/// Extract a required `u64` argument by name.
///
/// # Errors
/// Returns an error string if `name` is missing or not a JSON integer.
pub fn u64_arg(v: &Value, name: &str) -> Result<u64, String> {
    v.get(name)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("missing or non-integer arg '{name}'"))
}

/// Extract a required `usize` argument by name. Parsed as `u64` then cast.
pub fn usize_arg(v: &Value, name: &str) -> Result<usize, String> {
    u64_arg(v, name).map(|n| n as usize)
}

/// Extract an optional string argument by name. Returns `None` if missing or not a string.
pub fn opt_str_arg(v: &Value, name: &str) -> Option<String> {
    v.get(name).and_then(|v| v.as_str()).map(std::string::ToString::to_string)
}

/// Serialize a JSON value to bytes for the wire-protocol response.
///
/// # Errors
/// Returns an error string if the value cannot be serialized.
pub fn ok_json(val: Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&val).map_err(|e| format!("serialize result: {e}"))
}
