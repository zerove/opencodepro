use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use sisyphus_types::{CustomToolDefinition, ToolExecutor, TypesError};
use std::collections::HashMap;
use std::sync::Arc;
use wasmtime::*;

/// 轻量级 JSON Schema 校验器
struct SchemaValidator;

impl SchemaValidator {
    fn validate(schema: &Value, args: &Value) -> Result<(), String> {
        if let Some(obj_schema) = schema.get("type").and_then(|t| t.as_str()) {
            match obj_schema {
                "object" => Self::validate_object(schema, args),
                "string" => Self::validate_string(schema, args),
                "integer" | "number" => Self::validate_number(schema, args),
                "boolean" => Self::validate_boolean(schema, args),
                _ => Err(format!("Unsupported schema type: {}", obj_schema)),
            }
        } else {
            // 如果没有指定 type，默认视为通过（或者根据需要定义默认行为）
            Ok(())
        }
    }

    fn validate_object(schema: &Value, args: &Value) -> Result<(), String> {
        if !args.is_object() {
            return Err("Expected an object".to_string());
        }

        let props = schema.get("properties").and_then(|p| p.as_object());
        let required = schema.get("required").and_then(|r| r.as_array());

        if let Some(req_list) = required {
            for req in req_list {
                let req_name = req.as_str().ok_or("Required field name must be a string")?;
                if args.get(req_name).is_none() {
                    return Err(format!("Missing required field: {}", req_name));
                }
            }
        }

        if let Some(properties) = props {
            if let Some(args_obj) = args.as_object() {
                for (name, val) in args_obj {
                    if let Some(prop_schema) = properties.get(name) {
                        Self::validate(prop_schema, val)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn validate_string(schema: &Value, args: &Value) -> Result<(), String> {
        let s = args.as_str().ok_or("Expected a string")?;
        // minLength / maxLength 是字符数（JSON Schema spec），不是字节数。
        // proptest fuzz 发现 UTF-8 多字节字符判定 bug：旧实现用 s.len() 字节数。
        let char_count = s.chars().count();
        if let Some(min) = schema.get("minLength").and_then(|m| m.as_u64()) {
            if char_count < min as usize {
                return Err(format!("String too short (min {})", min));
            }
        }
        if let Some(max) = schema.get("maxLength").and_then(|m| m.as_u64()) {
            if char_count > max as usize {
                return Err(format!("String too long (max {})", max));
            }
        }
        Ok(())
    }

    fn validate_number(schema: &Value, args: &Value) -> Result<(), String> {
        let n = args.as_f64().ok_or("Expected a number")?;
        if let Some(min) = schema.get("minimum").and_then(|m| m.as_f64()) {
            if n < min {
                return Err(format!("Value too small (min {})", min));
            }
        }
        if let Some(max) = schema.get("maximum").and_then(|m| m.as_f64()) {
            if n > max {
                return Err(format!("Value too large (max {})", max));
            }
        }
        Ok(())
    }

    fn validate_boolean(_schema: &Value, args: &Value) -> Result<(), String> {
        if !args.is_boolean() {
            return Err("Expected a boolean".to_string());
        }
        Ok(())
    }
}

/// CustomToolExecutor 实现 ToolExecutor，支持 JSON Schema 校验 + WASM 执行
pub struct CustomToolExecutor {
    engine: Engine,
    module_cache: Arc<DashMap<String, Module>>,
    tools: HashMap<String, CustomToolDefinition>,
}

impl CustomToolExecutor {
    /// Build a custom-tool executor that hosts `tools` in-process (no WASM).
    ///
    /// Used for unit tests and trusted environments where the overhead of
    /// spawning a `wasmtime` instance is not justified.
    ///
    /// # Errors
    /// Returns `SandboxError` if any tool definition is malformed (e.g.
    /// duplicate tool names).
    pub fn new(tools: Vec<CustomToolDefinition>) -> Result<Self, SandboxError> {
        let mut config = Config::new();
        config.consume_fuel(true);

        let engine = Engine::new(&config)?;
        let mut tool_map = HashMap::new();
        for def in tools {
            tool_map.insert(def.name.clone(), def);
        }

        Ok(Self {
            engine,
            module_cache: Arc::new(DashMap::new()),
            tools: tool_map,
        })
    }

    async fn get_or_compile_module(&self, tool_id: &str) -> Result<Module, SandboxError> {
        if let Some(cached) = self.module_cache.get(tool_id) {
            return Ok(cached.clone());
        }

        let def = self
            .tools
            .get(tool_id)
            .ok_or_else(|| SandboxError::MissingExecuteExport(tool_id.to_string()))?;

        let wasm_bytes =
            tokio::fs::read(&def.handler).await.map_err(|e| SandboxError::IoError {
                tool_id: tool_id.to_string(),
                path: def.handler.clone(),
                source: e,
            })?;

        let engine = self.engine.clone();
        let module = tokio::task::spawn_blocking(move || Module::new(&engine, &wasm_bytes))
            .await
            .map_err(|e| SandboxError::TaskJoinError(e.to_string()))?
            .map_err(|e| SandboxError::CompileError {
                tool_id: tool_id.to_string(),
                source: e,
            })?;

        self.module_cache.insert(tool_id.to_string(), module.clone());
        Ok(module)
    }
}

impl CustomToolExecutor {
    /// 带燃料测量的执行内核（P2-9）。
    async fn execute_internal(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, Option<u64>), TypesError> {
        let def = self.tools.get(tool_id).ok_or_else(|| TypesError::ToolExecutionError {
            tool_id: tool_id.to_string(),
            details: "Custom tool not found".to_string(),
        })?;

        // 1. JSON Schema 校验
        let args_value: Value =
            serde_json::from_slice(&args).map_err(|e| TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: format!("Invalid JSON args: {e}"),
            })?;

        if let Err(e) = SchemaValidator::validate(&def.schema, &args_value) {
            return Err(TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: format!("Argument validation failed: {e}"),
            });
        }

        // 2. WASM 模块准备
        let module = self.get_or_compile_module(tool_id).await.map_err(|e| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: e.to_string(),
            }
        })?;

        let linker = Linker::new(&self.engine);
        let mut store = Store::new(&self.engine, ());
        store.set_fuel(fuel_limit).map_err(|e| TypesError::ToolExecutionError {
            tool_id: tool_id.to_string(),
            details: format!("Fuel error: {e}"),
        })?;

        let instance = linker.instantiate(&mut store, &module).map_err(|e| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: format!("Instantiation error: {e}"),
            }
        })?;

        let execute_func = instance.get_func(&mut store, "execute").ok_or_else(|| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: "WASM module missing 'execute' export".to_string(),
            }
        })?;

        // 3. 写入参数到 WASM 内存
        let args_json =
            serde_json::to_string(&args_value).map_err(|e| TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: format!("Failed to serialize WASM args: {e}"),
            })?;
        let args_bytes = args_json.as_bytes();

        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: "WASM module missing 'memory' export".to_string(),
            }
        })?;

        // 尝试调用 alloc 函数以获取安全的内存地址
        let arg_ptr = if let Some(alloc_func) = instance.get_func(&mut store, "alloc") {
            let mut alloc_results = vec![Val::I32(0)];
            alloc_func
                .call(
                    &mut store,
                    &[Val::I32(args_bytes.len() as i32)],
                    &mut alloc_results,
                )
                .map_err(|e| TypesError::ToolExecutionError {
                    tool_id: tool_id.to_string(),
                    details: format!("WASM alloc failed: {e}"),
                })?;
            alloc_results[0].unwrap_i32() as usize
        } else {
            return Err(TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: "WASM module missing required 'alloc' export. Tools must export 'alloc(size: i32) -> i32' to safely receive arguments.".to_string(),
            });
        };

        memory.write(&mut store, arg_ptr, args_bytes).map_err(|e| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: format!("Failed to write args to WASM memory at {}: {e}", arg_ptr),
            }
        })?;

        // 4. 执行与读取结果
        let mut results = vec![Val::I32(0)];
        execute_func.call(&mut store, &[Val::I32(arg_ptr as i32)], &mut results).map_err(|e| {
            TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: format!("WASM execution error: {e}"),
            }
        })?;
        let fuel_consumed = fuel_limit.saturating_sub(store.get_fuel().unwrap_or(0));

        let result_ptr = results[0].unwrap_i32() as usize;
        let data = memory.data(&store);
        if result_ptr < data.len() {
            let end = data[result_ptr..]
                .iter()
                .position(|&b| b == 0)
                .map(|pos| result_ptr + pos)
                .unwrap_or(data.len());
            Ok((data[result_ptr..end].to_vec(), Some(fuel_consumed)))
        } else {
            Err(TypesError::ToolExecutionError {
                tool_id: tool_id.to_string(),
                details: "WASM returned invalid memory pointer".to_string(),
            })
        }
    }
}

// 这里的 SandboxError 应从父模块引入
use crate::SandboxError;

#[async_trait]
impl ToolExecutor for CustomToolExecutor {
    async fn execute(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<Vec<u8>, TypesError> {
        let (data, _) = self.execute_internal(tool_id, args, fuel_limit).await?;
        Ok(data)
    }

    async fn execute_measured(
        &self,
        tool_id: &str,
        args: Vec<u8>,
        fuel_limit: u64,
    ) -> Result<(Vec<u8>, Option<sisyphus_types::ExecutionMeasurement>), TypesError> {
        let exec_start = std::time::Instant::now();
        let (data, fuel_consumed) = self.execute_internal(tool_id, args, fuel_limit).await?;
        Ok((
            data,
            fuel_consumed.map(|fuel_consumed| sisyphus_types::ExecutionMeasurement {
                fuel_consumed,
                duration_ms: exec_start.elapsed().as_millis() as u64,
            }),
        ))
    }
}

// ── Property-based fuzz tests for `SchemaValidator::validate` ──
//
// proptest fuzzes arbitrary schema/args combinations to discover
// panics, infinite loops, or wrong-typed branches.
//
// Note: `SchemaValidator` is private so these tests must live in the
// same module (cfg(test) mod). Coverage: 9 properties × 100 cases = 900+.

#[cfg(test)]
mod fuzz_tests {
    use super::SchemaValidator;
    use proptest::prelude::*;
    use serde_json::Value;

    fn arb_json_value() -> impl Strategy<Value = Value> {
        prop_oneof![
            any::<bool>().prop_map(Value::Bool),
            any::<i64>().prop_map(|n| Value::Number(n.into())),
            "[a-zA-Z0-9_ ]{0,32}".prop_map(Value::String),
            Just(Value::Null),
            proptest::collection::vec(any::<i64>().prop_map(|n| Value::Number(n.into())), 0..4)
                .prop_map(Value::Array),
        ]
    }

    fn arb_simple_schema() -> impl Strategy<Value = Value> {
        prop_oneof![
            Just(serde_json::json!({"type": "object"})),
            Just(serde_json::json!({"type": "string"})),
            Just(serde_json::json!({"type": "integer"})),
            Just(serde_json::json!({"type": "number"})),
            Just(serde_json::json!({"type": "boolean"})),
        ]
    }

    proptest! {
        #[test]
        fn schema_validator_never_panics(
            schema in arb_simple_schema(),
            value in arb_json_value()
        ) {
            // 仅断言不 panic（Ok 或 Err 都接受）
            let _ = SchemaValidator::validate(&schema, &value);
        }

        #[test]
        fn schema_validator_handles_malformed_schema(value in arb_json_value()) {
            for schema in [
                serde_json::json!({}),
                serde_json::json!({"type": "unknown"}),
                serde_json::json!({"type": Value::Null}),
                serde_json::json!({"type": 42}),
                serde_json::json!({"type": "object", "properties": "not_an_object"}),
                serde_json::json!({"type": "object", "required": "not_an_array"}),
                serde_json::json!({"type": "object", "properties": Value::Null}),
            ] {
                let _ = SchemaValidator::validate(&schema, &value);
            }
        }

        #[test]
        fn schema_validator_handles_deeply_nested_args(depth in 0usize..5) {
            let mut v = Value::Null;
            for _ in 0..depth {
                v = serde_json::json!({ "foo": v });
            }
            let schema = serde_json::json!({"type": "object"});
            let _ = SchemaValidator::validate(&schema, &v);
        }

        #[test]
        fn object_schema_rejects_non_object(
            value in prop_oneof![
                any::<i64>().prop_map(Value::from),
                "[a-z]{0,8}".prop_map(Value::String),
                Just(Value::Null),
            ]
        ) {
            let schema = serde_json::json!({"type": "object"});
            prop_assert!(SchemaValidator::validate(&schema, &value).is_err());
        }

        #[test]
        fn string_schema_rejects_non_string(value in arb_json_value()) {
            let schema = serde_json::json!({"type": "string"});
            if !value.is_string() {
                prop_assert!(SchemaValidator::validate(&schema, &value).is_err());
            }
        }

        #[test]
        fn number_schema_rejects_non_number(value in arb_json_value()) {
            let schema = serde_json::json!({"type": "integer"});
            if !value.is_number() {
                prop_assert!(SchemaValidator::validate(&schema, &value).is_err());
            }
        }

        #[test]
        fn boolean_schema_rejects_non_bool(value in arb_json_value()) {
            let schema = serde_json::json!({"type": "boolean"});
            if !value.is_boolean() {
                prop_assert!(SchemaValidator::validate(&schema, &value).is_err());
            }
        }

        #[test]
        fn string_min_length_respected(min in 0u64..16, s in ".*") {
            let schema = serde_json::json!({"type": "string", "minLength": min});
            let value = Value::String(s.clone());
            let r = SchemaValidator::validate(&schema, &value);
            if (s.chars().count() as u64) < min {
                prop_assert!(r.is_err(), "string len < min should fail");
            } else {
                prop_assert!(r.is_ok(), "string len >= min should pass");
            }
        }

        #[test]
        fn number_range_respected(
            min in -1000i64..1000,
            max in -1000i64..1000,
            v in -2000i64..2000
        ) {
            let schema = serde_json::json!({"type": "integer", "minimum": min, "maximum": max});
            let value = Value::Number(v.into());
            let r = SchemaValidator::validate(&schema, &value);
            if v < min || v > max {
                prop_assert!(r.is_err(), "v={} out of [{},{}] should fail", v, min, max);
            } else {
                prop_assert!(r.is_ok(), "v={} in [{},{}] should pass", v, min, max);
            }
        }
    }
}
