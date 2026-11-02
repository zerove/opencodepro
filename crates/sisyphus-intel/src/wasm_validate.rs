//! 生成 WASM 工具的静态校验。
//!
//! 沙箱的调用契约（见 `sisyphus-sandbox` 的 `execute_internal_opts`）要求模块：
//! 1. 导出 `memory`（host 从这里写输入、读输出）；
//! 2. 导出 `execute` 函数，返回指向 NUL 结尾字符串的 i32 指针。
//!
//! 缺任何一项，部署后必然运行期失败。这里在**部署前**读二进制判定，
//! 不依赖 LLM 自述、也不依赖沙箱真的跑一遍（后者是 `probe` 的职责）。
//!
//! 只解析到「导出段」为止，刻意不引入 wasmparser 依赖：需要的信息
//! （magic/version/导出表）在二进制里是定长编码，手写解析更小也更好审计。

/// 沙箱要求的宿主内存导出名。
pub const REQUIRED_MEMORY_EXPORT: &str = "memory";
/// 沙箱要求的入口函数导出名。
pub const REQUIRED_ENTRY_EXPORT: &str = "execute";

/// WASM 导出项的种类（与 spec 的 `external_kind` 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    /// 函数
    Func,
    /// 表
    Table,
    /// 线性内存
    Memory,
    /// 全局变量
    Global,
}

impl ExportKind {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Func),
            1 => Some(Self::Table),
            2 => Some(Self::Memory),
            3 => Some(Self::Global),
            _ => None,
        }
    }
}

/// 模块导出表里的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmExport {
    pub name: String,
    pub kind: ExportKind,
}

/// 模块声明的线性内存限制。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WasmMemoryLimits {
    /// 初始页数（1 页 = 64 KiB）。
    pub min_pages: u32,
    /// 上限页数（`None` 表示未声明上限）。
    pub max_pages: Option<u32>,
}

/// 静态校验的结论。
#[derive(Debug, Clone)]
pub struct WasmModuleInfo {
    /// 模块字节数。
    pub size_bytes: usize,
    /// 模块声明的全部导出项（顺序与二进制一致）。
    pub exports: Vec<WasmExport>,
    /// 内存段声明的初始/上限页数（没有内存段时为 `None`）。
    pub memory: Option<WasmMemoryLimits>,
}

impl WasmModuleInfo {
    /// 是否存在指定名字 + 种类的导出。
    pub fn has_export(&self, name: &str, kind: ExportKind) -> bool {
        self.exports.iter().any(|e| e.name == name && e.kind == kind)
    }
}

const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6d];
const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];
const SECTION_MEMORY: u8 = 5;
const SECTION_EXPORT: u8 = 7;

/// 解析 WASM 二进制，返回模块信息；不满足沙箱契约时返回 `Err(原因)`。
pub fn inspect_wasm(bytes: &[u8]) -> Result<WasmModuleInfo, String> {
    if bytes.len() < 8 {
        return Err(format!(
            "module is only {} bytes; a wasm header needs 8",
            bytes.len()
        ));
    }
    if bytes[..4] != WASM_MAGIC {
        return Err(format!("bad wasm magic: {:02x?}", &bytes[..4]));
    }
    if bytes[4..8] != WASM_VERSION {
        return Err(format!("unsupported wasm version: {:02x?}", &bytes[4..8]));
    }

    let mut cursor = 8usize;
    let mut exports: Option<Vec<WasmExport>> = None;
    let mut memory: Option<WasmMemoryLimits> = None;

    while cursor < bytes.len() {
        let section_id = bytes[cursor];
        cursor += 1;
        let (size, next) = read_leb_u32(bytes, cursor)?;
        cursor = next;
        let end = cursor
            .checked_add(size as usize)
            .ok_or_else(|| "section size overflows address space".to_string())?;
        if end > bytes.len() {
            return Err(format!(
                "section {section_id} claims {size} bytes but only {} remain",
                bytes.len() - cursor
            ));
        }

        if section_id == SECTION_EXPORT {
            exports = Some(parse_export_section(&bytes[cursor..end])?);
        } else if section_id == SECTION_MEMORY {
            memory = parse_memory_section(&bytes[cursor..end])?;
        }
        cursor = end;
    }

    let exports = exports.ok_or_else(|| "module has no export section".to_string())?;

    let info = WasmModuleInfo {
        size_bytes: bytes.len(),
        exports,
        memory,
    };

    if !info.has_export(REQUIRED_MEMORY_EXPORT, ExportKind::Memory) {
        return Err(format!(
            "missing `{REQUIRED_MEMORY_EXPORT}` memory export; the host cannot pass arguments or read results without it"
        ));
    }
    if !info.has_export(REQUIRED_ENTRY_EXPORT, ExportKind::Func) {
        return Err(format!(
            "missing `{REQUIRED_ENTRY_EXPORT}` function export; the sandbox has no entry point to call"
        ));
    }

    Ok(info)
}

/// 解析内存段，返回第一块内存的限制。
///
/// 一个模块可以有多个内存（multi-memory），沙箱只用导出的那一块；
/// 这里记录第一块的声明，用于「初始内存是否超出沙箱预算」的诊断。
fn parse_memory_section(body: &[u8]) -> Result<Option<WasmMemoryLimits>, String> {
    let mut cursor = 0usize;
    let (count, next) = read_leb_u32(body, cursor)?;
    cursor = next;
    if count == 0 {
        return Ok(None);
    }

    let flags = *body.get(cursor).ok_or_else(|| "memory entry is missing its flags".to_string())?;
    cursor += 1;
    let (min_pages, next) = read_leb_u32(body, cursor)?;
    cursor = next;

    // flags bit 0 表示声明了上限；bit 1 表示 64 位索引（本项目不支持）。
    if flags & 0x02 != 0 {
        return Err("64-bit memory indices are not supported by the sandbox".into());
    }
    let max_pages = if flags & 0x01 != 0 {
        let (max, _) = read_leb_u32(body, cursor)?;
        Some(max)
    } else {
        None
    };

    Ok(Some(WasmMemoryLimits {
        min_pages,
        max_pages,
    }))
}

fn parse_export_section(body: &[u8]) -> Result<Vec<WasmExport>, String> {
    let mut cursor = 0usize;
    let (count, next) = read_leb_u32(body, cursor)?;
    cursor = next;

    let mut exports = Vec::new();
    for index in 0..count {
        let (name_len, next) = read_leb_u32(body, cursor)?;
        cursor = next;
        let name_end = cursor
            .checked_add(name_len as usize)
            .ok_or_else(|| "export name length overflows".to_string())?;
        if name_end > body.len() {
            return Err(format!("export #{index} name runs past the section"));
        }
        let name = String::from_utf8(body[cursor..name_end].to_vec())
            .map_err(|e| format!("export #{index} name is not valid UTF-8: {e}"))?;
        cursor = name_end;

        let kind_byte = *body
            .get(cursor)
            .ok_or_else(|| format!("export #{index} (`{name}`) is missing its kind byte"))?;
        cursor += 1;
        let kind = ExportKind::from_byte(kind_byte)
            .ok_or_else(|| format!("export #{index} (`{name}`) has unknown kind {kind_byte}"))?;

        let (_index, next) = read_leb_u32(body, cursor)?;
        cursor = next;

        exports.push(WasmExport { name, kind });
    }

    Ok(exports)
}

/// 读一个无符号 LEB128（最多 5 字节 / 32 位）。
fn read_leb_u32(bytes: &[u8], start: usize) -> Result<(u32, usize), String> {
    let mut result: u32 = 0;
    let mut shift = 0u32;
    let mut cursor = start;

    loop {
        let byte =
            *bytes.get(cursor).ok_or_else(|| format!("truncated LEB128 at offset {start}"))?;
        cursor += 1;
        let chunk = u32::from(byte & 0x7f);
        if shift >= 32 || (shift == 28 && chunk > 0x0f) {
            return Err(format!("LEB128 at offset {start} does not fit in u32"));
        }
        result |= chunk << shift;
        if byte & 0x80 == 0 {
            return Ok((result, cursor));
        }
        shift += 7;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小可用模块：导出 `memory` 与 `execute`（与 `tools/echo_args.wasm` 同形）。
    fn minimal_module() -> Vec<u8> {
        let mut m = Vec::new();
        m.extend_from_slice(&WASM_MAGIC);
        m.extend_from_slice(&WASM_VERSION);

        // 类型段：() -> i32
        m.push(1);
        m.extend_from_slice(&[0x05, 0x01, 0x60, 0x00, 0x01, 0x7f]);
        // 函数段：一个函数，type 0
        m.push(3);
        m.extend_from_slice(&[0x02, 0x01, 0x00]);
        // 内存段：1 页，无上限
        m.push(5);
        m.extend_from_slice(&[0x03, 0x01, 0x00, 0x01]);
        // 导出段：memory + execute
        let mut export_body = vec![0x02];
        export_body.push(0x06);
        export_body.extend_from_slice(b"memory");
        export_body.push(0x02);
        export_body.push(0x00);
        export_body.push(0x07);
        export_body.extend_from_slice(b"execute");
        export_body.push(0x00);
        export_body.push(0x00);
        m.push(SECTION_EXPORT);
        m.push(export_body.len() as u8);
        m.extend_from_slice(&export_body);
        // 代码段：返回 0
        m.push(10);
        m.extend_from_slice(&[0x05, 0x01, 0x02, 0x00, 0x41, 0x00]);
        m
    }

    #[test]
    fn accepts_module_with_memory_and_execute() {
        let info = inspect_wasm(&minimal_module()).expect("minimal module must validate");
        assert!(info.has_export("memory", ExportKind::Memory));
        assert!(info.has_export("execute", ExportKind::Func));
        assert_eq!(info.exports.len(), 2);
        assert_eq!(
            info.memory,
            Some(WasmMemoryLimits {
                min_pages: 1,
                max_pages: None
            })
        );
    }

    #[test]
    fn records_declared_memory_limits() {
        // 与 minimal_module 同形，但内存段声明了上限：flags 1, min 2, max 8
        let mut m = Vec::new();
        m.extend_from_slice(&WASM_MAGIC);
        m.extend_from_slice(&WASM_VERSION);
        m.push(1);
        m.extend_from_slice(&[0x05, 0x01, 0x60, 0x00, 0x01, 0x7f]);
        m.push(3);
        m.extend_from_slice(&[0x02, 0x01, 0x00]);
        m.push(SECTION_MEMORY);
        m.extend_from_slice(&[0x04, 0x01, 0x01, 0x02, 0x08]);
        let mut export_body = vec![0x02];
        export_body.push(0x06);
        export_body.extend_from_slice(b"memory");
        export_body.extend_from_slice(&[0x02, 0x00]);
        export_body.push(0x07);
        export_body.extend_from_slice(b"execute");
        export_body.extend_from_slice(&[0x00, 0x00]);
        m.push(SECTION_EXPORT);
        m.push(export_body.len() as u8);
        m.extend_from_slice(&export_body);
        m.push(10);
        m.extend_from_slice(&[0x05, 0x01, 0x02, 0x00, 0x41, 0x00]);

        let info = inspect_wasm(&m).expect("module must validate");
        assert_eq!(
            info.memory,
            Some(WasmMemoryLimits {
                min_pages: 2,
                max_pages: Some(8)
            })
        );
    }

    #[test]
    fn rejects_bad_magic() {
        let mut m = minimal_module();
        m[0] = 0xff;
        let err = inspect_wasm(&m).expect_err("bad magic must be rejected");
        assert!(err.contains("magic"), "{err}");
    }

    #[test]
    fn rejects_truncated_header() {
        let err = inspect_wasm(b"\0asm").expect_err("short input must be rejected");
        assert!(err.contains("8"), "{err}");
    }

    #[test]
    fn rejects_module_without_export_section() {
        let mut m = Vec::new();
        m.extend_from_slice(&WASM_MAGIC);
        m.extend_from_slice(&WASM_VERSION);
        let err = inspect_wasm(&m).expect_err("no exports must be rejected");
        assert!(err.contains("no export section"), "{err}");
    }

    #[test]
    fn rejects_missing_execute_export() {
        // 只导出 memory
        let mut m = Vec::new();
        m.extend_from_slice(&WASM_MAGIC);
        m.extend_from_slice(&WASM_VERSION);
        m.push(5);
        m.extend_from_slice(&[0x03, 0x01, 0x00, 0x01]);
        let mut body = vec![0x01, 0x06];
        body.extend_from_slice(b"memory");
        body.extend_from_slice(&[0x02, 0x00]);
        m.push(SECTION_EXPORT);
        m.push(body.len() as u8);
        m.extend_from_slice(&body);

        let err = inspect_wasm(&m).expect_err("missing execute must be rejected");
        assert!(err.contains("execute"), "{err}");
    }

    #[test]
    fn rejects_memory_exported_as_function() {
        // 名字对但种类错（func 而非 memory）同样不可用
        let mut m = Vec::new();
        m.extend_from_slice(&WASM_MAGIC);
        m.extend_from_slice(&WASM_VERSION);
        m.push(1);
        m.extend_from_slice(&[0x04, 0x01, 0x60, 0x00, 0x00]);
        m.push(3);
        m.extend_from_slice(&[0x02, 0x01, 0x00]);
        let mut body = vec![0x01, 0x06];
        body.extend_from_slice(b"memory");
        body.extend_from_slice(&[0x00, 0x00]);
        m.push(SECTION_EXPORT);
        m.push(body.len() as u8);
        m.extend_from_slice(&body);

        let err = inspect_wasm(&m).expect_err("wrong export kind must be rejected");
        assert!(err.contains("memory"), "{err}");
    }

    #[test]
    fn leb_u32_rejects_truncation() {
        let err = read_leb_u32(&[0x80], 0).expect_err("truncated LEB must error");
        assert!(err.contains("truncated"), "{err}");
    }
}
