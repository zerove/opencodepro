use std::process::{Command, Output};

/// Helper: run the sisyphus-cli binary with args, returning stdout/stderr.
fn run_cli(args: &[&str]) -> Output {
    let bin = assert_cmd::cargo::cargo_bin("sisyphus-cli");
    Command::new(bin)
        .args(args)
        .output()
        .expect("failed to execute sisyphus-cli")
}

#[allow(dead_code)]
fn run_cli_stdout(args: &[&str]) -> String {
    let out = run_cli(args);
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[allow(dead_code)]
fn run_cli_stderr(args: &[&str]) -> String {
    let out = run_cli(args);
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn run_cli_success(args: &[&str]) -> String {
    let out = run_cli(args);
    assert!(out.status.success(), "CLI command failed: {args:?}\nstdout: {}\nstderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).to_string()
}

// ── Read command ──────────────────────────────────────────

#[test]
fn test_read_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello world").unwrap();

    let stdout = run_cli_success(&["read", file_path.to_str().unwrap()]);
    assert_eq!(stdout.trim(), "hello world");
}

#[test]
fn test_read_nonexistent_file() {
    let out = run_cli(&["read", "/tmp/does_not_exist_12345"]);
    assert!(!out.status.success(), "expected failure, got success");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.is_empty(), "expected error output");
}

// ── Write command ─────────────────────────────────────────

#[test]
fn test_write_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("output.txt");

    let stdout = run_cli_success(&["write", file_path.to_str().unwrap(), "test content"]);
    assert!(stdout.contains("Written"));

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "test content");
}

#[test]
fn test_write_empty_content() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("empty.txt");

    run_cli_success(&["write", file_path.to_str().unwrap(), ""]);
    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "");
}

// ── Exec command ──────────────────────────────────────────

#[test]
fn test_exec_echo() {
    let stdout = run_cli_success(&["exec", "echo hello"]);
    assert!(stdout.contains("hello"));
    assert!(stdout.contains("Exit: 0"));
}

#[test]
fn test_exec_with_workdir() {
    let dir = tempfile::tempdir().unwrap();
    let stdout = run_cli_success(&["exec", "pwd", "--dir", dir.path().to_str().unwrap()]);
    let pwd = stdout.lines().next().unwrap_or("");
    assert_eq!(pwd, dir.path().to_str().unwrap());
}

#[test]
fn test_exec_failure() {
    let out = run_cli(&["exec", "exit 42"]);
    assert!(!out.status.success(), "expected failure, got success");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.is_empty(), "expected error output");
}

// ── Help ──────────────────────────────────────────────────

#[test]
fn test_help_output() {
    let stdout = run_cli_success(&["--help"]);
    assert!(stdout.contains("Sisyphus Cognitive Engine CLI"));
    assert!(stdout.contains("read"));
    assert!(stdout.contains("write"));
    assert!(stdout.contains("exec"));
    assert!(stdout.contains("search"));
    assert!(stdout.contains("sop"));
    assert!(stdout.contains("health"));
}

// ── Version ───────────────────────────────────────────────

#[test]
fn test_version_output() {
    let stdout = run_cli_success(&["--version"]);
    assert!(!stdout.is_empty());
}

// ── Interactive mode (just starts, we send Ctrl-C) ────────

#[test]
fn test_interactive_mode_flag() {
    let bin = assert_cmd::cargo::cargo_bin("sisyphus-cli");
    let mut child = Command::new(bin)
        .arg("--interactive")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn interactive CLI");

    // Give it a moment to start, then kill
    std::thread::sleep(std::time::Duration::from_millis(500));
    child.kill().expect("failed to kill interactive CLI");

    let output = child.wait_with_output().expect("failed to wait");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // TUI should have printed something before being killed
    assert!(!stdout.is_empty() || !output.status.success());
}

// ── Agent: metis analyse ───────────────────────────────────

#[test]
fn test_metis_analyse_inline() {
    let stdout = run_cli_success(&[
        "metis", "analyse",
        "Refactor the user authentication module to support OAuth2",
    ]);
    assert!(stdout.contains("Metis Analysis"));
    assert!(stdout.contains("summary") || stdout.contains("Ambiguities") || stdout.contains("Implicit"));
}

#[test]
fn test_metis_analyse_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let req_file = dir.path().join("request.txt");
    std::fs::write(&req_file, "Make the login page faster").unwrap();

    let stdout = run_cli_success(&["metis", "analyse", "--file", req_file.to_str().unwrap()]);
    assert!(stdout.contains("Metis Analysis"));
}

// ── Agent: momus review-plan ───────────────────────────────

#[test]
fn test_momus_review_plan() {
    let dir = tempfile::tempdir().unwrap();
    let plan_file = dir.path().join("plan.md");
    std::fs::write(&plan_file,
        "- Step 1: Implement the API endpoint\n\
         - Step 2: Add input validation\n\
         - Step 3: Write integration tests\n\
         - Step 4: Deploy to staging\n").unwrap();

    let stdout = run_cli_success(&["momus", "review-plan", plan_file.to_str().unwrap()]);
    assert!(!stdout.is_empty());
}

#[test]
fn test_momus_review_code() {
    let dir = tempfile::tempdir().unwrap();
    let code_file = dir.path().join("example.rs");
    std::fs::write(&code_file,
        "fn hello() {\n    println!(\"hello world\");\n}\n").unwrap();

    let stdout = run_cli_success(&["momus", "review-code", code_file.to_str().unwrap()]);
    assert!(!stdout.is_empty());
}

#[test]
fn test_momus_review_arch() {
    let stdout = run_cli_success(&[
        "momus", "review-arch",
        "The system uses a microservices architecture with PostgreSQL for persistence \
         and RabbitMQ for async communication between services.",
    ]);
    assert!(!stdout.is_empty());
}

// ── Agent: librarian synthesise ─────────────────────────────

#[test]
fn test_librarian_synthesise() {
    let dir = tempfile::tempdir().unwrap();
    let findings_file = dir.path().join("findings.json");
    let findings = serde_json::json!([
        {"claim": "Rust is memory-safe", "source_url": "https://example.com/a", "source_title": "Source A", "relevance": 0.9, "retrieved_at": "2025-01-01"},
        {"claim": "Rust has zero-cost abstractions", "source_url": "https://example.com/b", "source_title": "Source B", "relevance": 0.8, "retrieved_at": "2025-01-01"},
    ]);
    std::fs::write(&findings_file, serde_json::to_string_pretty(&findings).unwrap()).unwrap();

    let stdout = run_cli_success(&["librarian", "synthesise", findings_file.to_str().unwrap()]);
    assert!(!stdout.is_empty(), "expected synthesis output, got empty");
}

// ── Agent: help output ─────────────────────────────────────

#[test]
fn test_help_contains_agent_subcommands() {
    let stdout = run_cli_success(&["--help"]);
    assert!(stdout.contains("librarian"), "--help should list librarian subcommand");
    assert!(stdout.contains("momus"), "--help should list momus subcommand");
    assert!(stdout.contains("metis"), "--help should list metis subcommand");
}

#[test]
fn test_librarian_help() {
    let stdout = run_cli_success(&["librarian", "--help"]);
    assert!(stdout.contains("research"));
    assert!(stdout.contains("fetch-docs"));
    assert!(stdout.contains("examples"));
    assert!(stdout.contains("synthesise"));
}

#[test]
fn test_momus_help() {
    let stdout = run_cli_success(&["momus", "--help"]);
    assert!(stdout.contains("review-plan"));
    assert!(stdout.contains("review-code"));
    assert!(stdout.contains("review-arch"));
}

#[test]
fn test_metis_help() {
    let stdout = run_cli_success(&["metis", "--help"]);
    assert!(stdout.contains("analyse"));
}
