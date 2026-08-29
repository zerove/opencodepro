//! 用于自主工具生成的 WASM 编译器。

use std::fs;
use std::path::{Path, PathBuf};

pub struct WasmCompiler {}

impl Default for WasmCompiler {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmCompiler {
    /// New.
    pub fn new() -> Self {
        Self {}
    }

    /// 将 Rust 源代码编译为 WASM 二进制。
    pub async fn compile(
        &self,
        tool_name: &str,
        _code: &str,
    ) -> Result<PathBuf, crate::IntelError> {
        // 用于测试的 MOCK 实现：跳过实际 cargo build
        let persistent_dir = Path::new("target/sisyphus_tools");
        fs::create_dir_all(persistent_dir).map_err(|e| {
            crate::IntelError::Internal(format!("Failed to create persistent tools dir: {e}"))
        })?;

        let final_path = persistent_dir.join(format!("{}.wasm", tool_name));
        fs::write(&final_path, b"MOCK WASM BINARY").map_err(|e| {
            crate::IntelError::Internal(format!("Failed to write mock WASM binary: {e}"))
        })?;

        Ok(final_path)
    }
}
