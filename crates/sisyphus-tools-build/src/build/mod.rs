use sisyphus_tools_core::{ToolError, ToolResult};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::Path;
use std::process::Command;

// ── Re-export submodules ──────────────────────────────

pub mod frontend;
pub mod golang;
pub mod java;
pub mod node;
pub mod python;
pub mod rust;

// ── Structured return types ────────────────────────────────

/// Parsed test results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResults {
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
    pub measured: usize,
    pub filtered_out: usize,
    pub failures: Vec<TestFailure>,
    pub raw_stdout: String,
    pub raw_stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Test Failure.
pub struct TestFailure {
    pub name: String,
}

/// A single compiler diagnostic (error or warning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub file: Option<String>,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub level: String,        // "error" or "warning"
    pub code: Option<String>, // e.g. "E0425"
    pub message: String,
}

/// Parsed compiler output (errors + warnings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    pub errors: Vec<Diag>,
    pub warnings: Vec<Diag>,
    pub raw_stdout: String,
    pub raw_stderr: String,
    pub success: bool,
}

// ── LanguageBackend trait ─────────────────────────────

/// A language-agnostic build system backend.
///
/// Each implementation provides build/check/test/lint/fix/format/doc
/// operations for a specific language ecosystem.
pub trait LanguageBackend: Debug + Send + Sync {
    /// Human-readable backend name, e.g. "rust", "node", "python".
    fn name(&self) -> &str;

    /// Type-check / compile-check (tsc --noEmit, cargo check, go vet, mypy ...)
    fn check(&self, pkg: Option<&str>, all: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput>;

    /// Compile build (cargo build, tsc, go build, mvn compile ...)
    fn build(
        &self,
        pkg: Option<&str>,
        release: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput>;

    /// Run tests (cargo test, pytest, vitest, go test ...)
    fn test(
        &self,
        pkg: Option<&str>,
        name: Option<&str>,
        all: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<TestResults>;

    /// Lint / static analysis (clippy, eslint, ruff, staticcheck, checkstyle ...)
    fn lint(&self, pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<BuildOutput>;

    /// Auto-apply fixes (cargo fix, eslint --fix, ruff --fix, gofmt -w ...)
    fn fix(
        &self,
        pkg: Option<&str>,
        apply_lint_fixes: bool,
        cwd: Option<&Path>,
    ) -> ToolResult<BuildOutput>;

    /// Check formatting (cargo fmt --check, prettier --check, ruff format --check ...)
    fn fmt_check(&self, cwd: Option<&Path>) -> ToolResult<bool>;

    /// Format files in-place (cargo fmt, prettier --write, ruff format ...)
    fn fmt_write(&self, cwd: Option<&Path>) -> ToolResult<()>;

    /// Generate documentation (cargo doc, typedoc, pdoc, godoc ...)
    fn doc(&self, pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<String>;

    /// Run the project (cargo run, npm start, python main.py, go run . ...).
    /// Returns the combined stdout/stderr output.
    /// Default implementation returns an error indicating "run" is not supported.
    fn run(&self, _args: &[&str], _cwd: Option<&Path>) -> ToolResult<String> {
        Err(ToolError::Generic(format!(
            "`run` not supported for {} backend (no standard run command)",
            self.name()
        )))
    }
}

// ── Auto-detection ────────────────────────────────────

/// Detect the language backend for a project directory based on config files.
///
/// Checks for: `Cargo.toml`, `package.json`, `pyproject.toml`/`setup.py`,
/// `go.mod`, `pom.xml`/`build.gradle`, or falls back to `RustBackend`.
pub fn detect_backend(cwd: &Path) -> ToolResult<Box<dyn LanguageBackend>> {
    if cwd.join("Cargo.toml").exists() {
        return Ok(Box::new(rust::RustBackend));
    }
    if cwd.join("package.json").exists() {
        return Ok(Box::new(node::NodeBackend::new()?));
    }
    if cwd.join("pyproject.toml").exists()
        || cwd.join("setup.py").exists()
        || cwd.join("setup.cfg").exists()
    {
        return Ok(Box::new(python::PythonBackend::new()?));
    }
    if cwd.join("go.mod").exists() {
        return Ok(Box::new(golang::GoBackend));
    }
    if cwd.join("pom.xml").exists() {
        return Ok(Box::new(java::JavaBackend::new_maven()));
    }
    if cwd.join("build.gradle").exists() || cwd.join("build.gradle.kts").exists() {
        return Ok(Box::new(java::JavaBackend::new_gradle()));
    }
    // Fallback: Rust
    Ok(Box::new(rust::RustBackend))
}

// ── Generic command runner ────────────────────────────

/// Run an arbitrary command, returning (stdout, stderr, success).
pub fn run_command(
    cmd: &str,
    args: &[&str],
    cwd: Option<&Path>,
) -> ToolResult<(String, String, bool)> {
    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ToolError::Generic(format!("`{cmd}` executable not found — is it installed?"))
        } else {
            ToolError::Io(e)
        }
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((stdout, stderr, output.status.success()))
}

/// Run `cargo` with the given arguments.
pub fn cargo_raw(args: &[&str], cwd: Option<&Path>) -> ToolResult<(String, String, bool)> {
    run_command("cargo", args, cwd)
}

// ── Diagnostic merging helpers (language-agnostic) ────

/// Merge two sets of diagnostics, deduplicating by (file, line, message).
/// Returns a single Vec sorted by (file, line, col).
pub fn merge_diagnostics(primary: &[Diag], secondary: &[Diag]) -> Vec<Diag> {
    let mut all: Vec<Diag> = primary.to_vec();
    let existing: Vec<(Option<String>, Option<usize>, String)> = all
        .iter()
        .map(|d| (d.file.clone(), d.line, d.message.clone()))
        .collect();
    for d in secondary {
        let key = (d.file.clone(), d.line, d.message.clone());
        if !existing.contains(&key) {
            all.push(d.clone());
        }
    }
    all.sort_by(|a, b| {
        a.file
            .as_deref()
            .unwrap_or("")
            .cmp(b.file.as_deref().unwrap_or(""))
            .then(a.line.unwrap_or(0).cmp(&b.line.unwrap_or(0)))
            .then(a.col.unwrap_or(0).cmp(&b.col.unwrap_or(0)))
    });
    all
}

/// Group diagnostics by file, returning (file → count) sorted by file name.
pub fn group_diagnostics_by_file(diags: &[Diag], level_filter: &str) -> Vec<(String, usize)> {
    let mut map: BTreeMap<String, usize> = BTreeMap::new();
    for d in diags {
        if d.level == level_filter {
            let f = d.file.clone().unwrap_or_else(|| "<unknown>".to_string());
            *map.entry(f).or_insert(0) += 1;
        }
    }
    map.into_iter().collect()
}

/// Count diagnostics matching a level.
pub fn count_diagnostics(diags: &[Diag], level: &str) -> usize {
    diags.iter().filter(|d| d.level == level).count()
}

// ── Backward-compatible free functions ────────────────
// These delegate to RustBackend so existing callers (diagnose.rs, CLI) work unchanged.

/// Run `cargo check`.
pub fn check(pkg: Option<&str>, all: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
    let backend = rust::RustBackend;
    backend.check(pkg, all, cwd)
}

/// Run `cargo build`.
pub fn build(pkg: Option<&str>, release: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
    let backend = rust::RustBackend;
    backend.build(pkg, release, cwd)
}

/// Run `cargo test`.
pub fn test(
    pkg: Option<&str>,
    name: Option<&str>,
    all: bool,
    cwd: Option<&Path>,
) -> ToolResult<TestResults> {
    let backend = rust::RustBackend;
    backend.test(pkg, name, all, cwd)
}

/// Run `cargo clippy`.
pub fn clippy(pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
    let backend = rust::RustBackend;
    backend.lint(pkg, cwd)
}

/// Run `cargo fix`.
pub fn fix(pkg: Option<&str>, clippy_fixes: bool, cwd: Option<&Path>) -> ToolResult<BuildOutput> {
    let backend = rust::RustBackend;
    backend.fix(pkg, clippy_fixes, cwd)
}

/// Run `cargo fmt --check`.
pub fn fmt_check(cwd: Option<&Path>) -> ToolResult<bool> {
    let backend = rust::RustBackend;
    backend.fmt_check(cwd)
}

/// Run `cargo doc`.
pub fn doc(pkg: Option<&str>, cwd: Option<&Path>) -> ToolResult<String> {
    let backend = rust::RustBackend;
    backend.doc(pkg, cwd)
}
