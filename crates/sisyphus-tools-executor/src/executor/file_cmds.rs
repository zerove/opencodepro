use crate::executor::args::{ok_json, parse_args, path_arg, str_arg, usize_arg};
use serde_json::json;
use std::path::Path;

// ── file command implementations ─────────────────────────────────

pub fn cmd_file_read(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let content = sisyphus_tools_system::file::read(Path::new(&path)).map_err(|e| e.to_string())?;
    ok_json(json!({"content": content}))
}

pub fn cmd_file_write(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let content = str_arg(&v, "content")?;
    sisyphus_tools_system::file::write(Path::new(&path), &content).map_err(|e| e.to_string())?;
    ok_json(json!({"bytes_written": content.len()}))
}

pub fn cmd_file_edit(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let old = str_arg(&v, "old")?;
    let new = str_arg(&v, "new")?;
    sisyphus_tools_system::file::edit(Path::new(&path), &old, &new).map_err(|e| e.to_string())?;
    ok_json(json!({"replaced": true}))
}

pub fn cmd_file_edit_all(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let old = str_arg(&v, "old")?;
    let new = str_arg(&v, "new")?;
    let count = sisyphus_tools_system::file::edit_all(Path::new(&path), &old, &new).map_err(|e| e.to_string())?;
    ok_json(json!({"count": count}))
}

pub fn cmd_file_read_lines(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let path = path_arg(&v, "path")?;
    let offset = usize_arg(&v, "offset").unwrap_or(0);
    let limit = usize_arg(&v, "limit").unwrap_or(2000);
    let content =
        sisyphus_tools_system::file::read_lines(Path::new(&path), offset, limit).map_err(|e| e.to_string())?;
    ok_json(json!({"content": content}))
}

pub fn cmd_glob(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let pattern = str_arg(&v, "pattern")?;
    let root = path_arg(&v, "root")?;
    let files = sisyphus_tools_system::file::glob(&pattern, Path::new(&root)).map_err(|e| e.to_string())?;
    let paths: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    ok_json(json!({"files": paths}))
}

pub fn cmd_grep(args: Vec<u8>) -> Result<Vec<u8>, String> {
    let v = parse_args(&args)?;
    let pattern = str_arg(&v, "pattern")?;
    let path = path_arg(&v, "path")?;
    let include = crate::executor::args::opt_str_arg(&v, "include");
    let matches = sisyphus_tools_system::file::grep(&pattern, Path::new(&path), include.as_deref())
        .map_err(|e| e.to_string())?;
    let list: Vec<serde_json::Value> = matches
        .iter()
        .map(|m| {
            json!({
                "file": m.file.to_string_lossy(),
                "line": m.line,
                "content": m.content,
            })
        })
        .collect();
    ok_json(json!({"matches": list}))
}
