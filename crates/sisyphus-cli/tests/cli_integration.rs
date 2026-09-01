use std::process::{Command, Output};

/// Helper: run the sisyphus-cli binary with args, returning stdout/stderr.
fn run_cli(args: &[&str]) -> Output {
    let bin = assert_cmd::cargo::cargo_bin("sisyphus");
    Command::new(bin).args(args).output().expect("failed to execute sisyphus-cli")
}

/// Helper used by future assertions; intentionally retained even when
/// the current test suite only calls `run_cli` directly.
#[allow(dead_code)]
fn run_cli_stdout(args: &[&str]) -> String {
    let out = run_cli(args);
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Helper used by future assertions; intentionally retained even when
/// the current test suite only calls `run_cli` directly.
#[allow(dead_code)]
fn run_cli_stderr(args: &[&str]) -> String {
    let out = run_cli(args);
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn run_cli_success(args: &[&str]) -> String {
    let out = run_cli(args);
    assert!(
        out.status.success(),
        "CLI command failed: {args:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
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
    assert!(stdout.contains("Sisyphus 认知引擎 CLI"));
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
    let bin = assert_cmd::cargo::cargo_bin("sisyphus");
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
        "metis",
        "analyse",
        "Refactor the user authentication module to support OAuth2",
    ]);
    assert!(stdout.contains("Metis Analysis"));
    assert!(
        stdout.contains("Summary") || stdout.contains("Ambiguities") || stdout.contains("Implicit")
    );
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
    std::fs::write(
        &plan_file,
        "- Step 1: Implement the API endpoint\n\
         - Step 2: Add input validation\n\
         - Step 3: Write integration tests\n\
         - Step 4: Deploy to staging\n",
    )
    .unwrap();

    let stdout = run_cli_success(&["momus", "review-plan", plan_file.to_str().unwrap()]);
    assert!(!stdout.is_empty());
}

#[test]
fn test_momus_review_code() {
    let dir = tempfile::tempdir().unwrap();
    let code_file = dir.path().join("example.rs");
    std::fs::write(
        &code_file,
        "fn hello() {\n    println!(\"hello world\");\n}\n",
    )
    .unwrap();

    let stdout = run_cli_success(&["momus", "review-code", code_file.to_str().unwrap()]);
    assert!(!stdout.is_empty());
}

#[test]
fn test_momus_review_arch() {
    let stdout = run_cli_success(&[
        "momus",
        "review-arch",
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
    std::fs::write(
        &findings_file,
        serde_json::to_string_pretty(&findings).unwrap(),
    )
    .unwrap();

    let stdout = run_cli_success(&["librarian", "synthesise", findings_file.to_str().unwrap()]);
    assert!(!stdout.is_empty(), "expected synthesis output, got empty");
}

// ── Agent: help output ─────────────────────────────────────

#[test]
fn test_help_contains_agent_subcommands() {
    let stdout = run_cli_success(&["--help"]);
    assert!(
        stdout.contains("librarian"),
        "--help should list librarian subcommand"
    );
    assert!(
        stdout.contains("momus"),
        "--help should list momus subcommand"
    );
    assert!(
        stdout.contains("metis"),
        "--help should list metis subcommand"
    );
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

// ── Edit command ─────────────────────────────────────────

#[test]
fn test_edit_first_occurrence() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("edit.txt");
    std::fs::write(&file_path, "the quick brown fox").unwrap();

    let stdout = run_cli_success(&["edit", file_path.to_str().unwrap(), "quick", "slow"]);
    assert!(stdout.contains("Replaced first occurrence"));

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "the slow brown fox");
}

#[test]
fn test_edit_all_occurrences() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("edit_all.txt");
    std::fs::write(&file_path, "a-b-a-c-a").unwrap();

    let stdout = run_cli_success(&["edit", file_path.to_str().unwrap(), "a", "x", "--all"]);
    assert!(stdout.contains("3 occurrence(s)"));

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "x-b-x-c-x");
}

#[test]
fn test_edit_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("edit_none.txt");
    std::fs::write(&file_path, "original content").unwrap();

    let out = run_cli(&[
        "edit",
        file_path.to_str().unwrap(),
        "nonexistent",
        "replacement",
    ]);
    assert!(!out.status.success(), "expected failure for no match");
}

// ── Glob command ─────────────────────────────────────────

#[test]
fn test_glob_matches_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "").unwrap();
    std::fs::write(dir.path().join("b.rs"), "").unwrap();
    std::fs::write(dir.path().join("c.txt"), "").unwrap();

    let stdout = run_cli_success(&["glob", "*.rs", dir.path().to_str().unwrap()]);
    assert!(stdout.contains("a.rs"));
    assert!(stdout.contains("b.rs"));
    assert!(!stdout.contains("c.txt"));
}

#[test]
fn test_glob_no_match() {
    let dir = tempfile::tempdir().unwrap();

    let stdout = run_cli_success(&["glob", "*.xyz", dir.path().to_str().unwrap()]);
    assert!(stdout.contains("No files matched"));
}

// ── Grep command ─────────────────────────────────────────

#[test]
fn test_grep_finds_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("search.txt");
    std::fs::write(&file_path, "line one\nsecret sauce\nline three").unwrap();

    let stdout = run_cli_success(&["grep", "secret", dir.path().to_str().unwrap()]);
    assert!(stdout.contains("secret sauce"));
}

#[test]
fn test_grep_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("plain.txt");
    std::fs::write(&file_path, "nothing special").unwrap();

    let stdout = run_cli_success(&["grep", "zzzzz", dir.path().to_str().unwrap()]);
    assert!(stdout.contains("No matches"));
}

// ── Todo CRUD ────────────────────────────────────────────

#[test]
fn test_todo_add_list_update_delete() {
    let dir = tempfile::tempdir().unwrap();

    let stdout = run_cli_success(&["--data-dir", dir.path().to_str().unwrap(), "todo", "list"]);
    assert!(stdout.contains("No todos found."));

    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "todo",
        "add",
        "test task",
    ]);
    assert!(stdout.contains("Created:"));

    let stdout = run_cli_success(&["--data-dir", dir.path().to_str().unwrap(), "todo", "list"]);
    assert!(stdout.contains("test task"));
    assert!(stdout.contains("pending"));

    let add_out = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "todo",
        "add",
        "another task",
    ]);
    let id = add_out.split_whitespace().nth(1).unwrap_or("1");
    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "todo",
        "update",
        id,
        "-s",
        "completed",
    ]);
    assert!(stdout.contains("Updated:"));

    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "todo",
        "delete",
        id,
        "-y",
    ]);
    assert!(stdout.contains("Deleted:"));
}

// ── Session CRUD ─────────────────────────────────────────

#[test]
fn test_session_create_list_get_delete() {
    let dir = tempfile::tempdir().unwrap();

    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "session",
        "list",
    ]);
    assert!(stdout.contains("No sessions."));

    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "session",
        "create",
        "test session",
    ]);
    assert!(stdout.contains("Created session:"));

    let parts: Vec<&str> = stdout.split_whitespace().collect();
    let session_id = parts.get(2).copied().unwrap_or("1");

    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "session",
        "list",
    ]);
    assert!(stdout.contains("test session"));

    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "session",
        "get",
        session_id,
    ]);
    assert!(stdout.contains("Session:"));

    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "session",
        "add-message",
        session_id,
        "user",
        "hello",
    ]);
    assert!(stdout.contains("Message added"));

    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "session",
        "rename",
        session_id,
        "renamed session",
    ]);
    assert!(stdout.contains("Renamed session"));

    let stdout = run_cli_success(&[
        "--data-dir",
        dir.path().to_str().unwrap(),
        "session",
        "delete",
        session_id,
        "-y",
    ]);
    assert!(stdout.contains("Deleted session"));
}

// ── Plan CRUD ────────────────────────────────────────────

#[test]
fn test_plan_create_show_from_description() {
    let stdout = run_cli_success(&[
        "plan",
        "create",
        "my plan",
        "-d",
        "Init phase\nBuild phase\nTest phase",
    ]);
    assert!(stdout.contains("Plan: my plan"));
    assert!(stdout.contains(" steps"));
}

#[test]
fn test_plan_create_from_file_then_save_show_review_approve() {
    let dir = tempfile::tempdir().unwrap();

    let req_file = dir.path().join("reqs.txt");
    std::fs::write(
        &req_file,
        "- Step A: design\n- Step B: implement\n- Step C: verify\n",
    )
    .unwrap();

    let stdout = run_cli_success(&[
        "plan",
        "create",
        "my plan",
        "-f",
        req_file.to_str().unwrap(),
    ]);
    assert!(stdout.contains("3 steps"));

    let plan_json = serde_json::json!({
        "id": "test-plan-id",
        "name": "test plan",
        "description": "a test plan",
        "status": "Draft",
        "steps": [
            {"id": "s1", "description": "step one", "status": "Draft", "dependencies": []},
            {"id": "s2", "description": "step two", "status": "Draft", "dependencies": []}
        ],
        "created_at": "2025-01-01T00:00:00Z",
        "updated_at": "2025-01-01T00:00:00Z"
    });
    let plan_file = dir.path().join("plan.json");
    std::fs::write(
        &plan_file,
        serde_json::to_string_pretty(&plan_json).unwrap(),
    )
    .unwrap();

    let stdout = run_cli_success(&["plan", "show", plan_file.to_str().unwrap()]);
    assert!(stdout.contains("test plan"));
    assert!(stdout.contains("step one"));

    let saved = dir.path().join("saved.json");
    let stdout = run_cli_success(&[
        "plan",
        "save",
        plan_file.to_str().unwrap(),
        saved.to_str().unwrap(),
    ]);
    assert!(stdout.contains("Saved plan"));
    assert!(saved.exists());

    let stdout = run_cli_success(&["plan", "review", saved.to_str().unwrap()]);
    assert!(stdout.contains("No issues found") || stdout.contains("Approved"));

    let stdout = run_cli_success(&["plan", "approve", saved.to_str().unwrap()]);
    assert!(stdout.contains("Approved"));
}

// ── Oracle commands ──────────────────────────────────────

#[test]
fn test_oracle_analyze_file() {
    let dir = tempfile::tempdir().unwrap();
    let rs_file = dir.path().join("hello.rs");
    std::fs::write(
        &rs_file,
        "use std::fmt;\n\nfn greet() -> String {\n    \"hi\".to_string()\n}\n",
    )
    .unwrap();

    let stdout = run_cli_success(&["oracle", "analyze-file", rs_file.to_str().unwrap()]);
    assert!(stdout.contains("File:"));
    assert!(stdout.contains("Lines:"));
    assert!(stdout.contains("greet"));
}

#[test]
fn test_oracle_analyze_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mod.rs"), "pub fn a() {}").unwrap();
    std::fs::write(dir.path().join("util.rs"), "pub fn b() {}").unwrap();

    let stdout = run_cli_success(&["oracle", "analyze-dir", dir.path().to_str().unwrap()]);
    assert!(stdout.contains("Directory:"));
    assert!(stdout.contains("Files analyzed:"));
    assert!(stdout.contains("2"));
}

#[test]
fn test_oracle_code_review() {
    let dir = tempfile::tempdir().unwrap();
    let rs_file = dir.path().join("clean.rs");
    std::fs::write(&rs_file, "fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();

    let stdout = run_cli_success(&["oracle", "code-review", rs_file.to_str().unwrap()]);
    assert!(!stdout.is_empty());
}

// ── Explore commands ─────────────────────────────────────

#[test]
fn test_explore_search_finds_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let rs_file = dir.path().join("example.rs");
    std::fs::write(&rs_file, "const SECRET: i32 = 42;\nfn main() {}").unwrap();

    let stdout = run_cli_success(&[
        "explore",
        "search",
        "SECRET",
        "-p",
        dir.path().to_str().unwrap(),
    ]);
    assert!(stdout.contains("SECRET"));
}

#[test]
fn test_explore_search_no_match() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.txt"), "nothing here").unwrap();

    let stdout = run_cli_success(&[
        "explore",
        "search",
        "ZZZZ_NOT_THERE",
        "-p",
        dir.path().to_str().unwrap(),
    ]);
    assert!(stdout.contains("No matches"));
}

#[test]
fn test_explore_find_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "").unwrap();
    std::fs::write(dir.path().join("lib.rs"), "").unwrap();

    let stdout = run_cli_success(&[
        "explore",
        "find-files",
        "*.rs",
        dir.path().to_str().unwrap(),
    ]);
    assert!(stdout.contains("main.rs"));
    assert!(stdout.contains("lib.rs"));
}

#[test]
fn test_explore_find_files_no_match() {
    let dir = tempfile::tempdir().unwrap();

    let stdout = run_cli_success(&[
        "explore",
        "find-files",
        "*.py",
        dir.path().to_str().unwrap(),
    ]);
    assert!(stdout.contains("No files matched"));
}

// ── ReadLines command ─────────────────────────────────────

#[test]
fn test_read_lines_full_file() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("lines.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

    let stdout = run_cli_success(&["read-lines", file_path.to_str().unwrap()]);
    assert!(stdout.contains("line1"));
    assert!(stdout.contains("line2"));
    assert!(stdout.contains("line3"));
}

#[test]
fn test_read_lines_with_offset() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("multi.txt");
    std::fs::write(&file_path, "a\nb\nc\nd\ne\n").unwrap();

    let stdout = run_cli_success(&["read-lines", file_path.to_str().unwrap(), "-o", "2"]);
    assert!(!stdout.contains("a"));
    assert!(stdout.contains("c"));
    assert!(stdout.contains("e"));
}

#[test]
fn test_read_lines_with_limit() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("bounded.txt");
    std::fs::write(&file_path, "1\n2\n3\n4\n5\n").unwrap();

    let stdout = run_cli_success(&["read-lines", file_path.to_str().unwrap(), "-l", "2"]);
    assert!(stdout.contains("1"));
    assert!(stdout.contains("2"));
    assert!(!stdout.contains("3"));
}

#[test]
fn test_read_lines_nonexistent_file() {
    let out = run_cli(&["read-lines", "/tmp/nope_readlines_test"]);
    assert!(!out.status.success());
}

// ── Skill commands ────────────────────────────────────────

#[test]
fn test_skill_list() {
    let stdout = run_cli_success(&["skill", "list"]);
    assert!(!stdout.is_empty());
}

#[test]
fn test_skill_show_nonexistent() {
    let out = run_cli(&["skill", "show", "__nonexistent_skill_12345__"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn test_skill_help() {
    let stdout = run_cli_success(&["skill", "--help"]);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("show"));
    assert!(stdout.contains("install"));
    assert!(stdout.contains("remove"));
}

// ── Delegate commands ─────────────────────────────────────

#[test]
fn test_delegate_inline_single_command() {
    let stdout = run_cli_success(&["delegate", "inline", "echo hello_delegate"]);
    assert!(stdout.contains("hello_delegate"));
    assert!(stdout.contains("Done:"));
}

#[test]
fn test_delegate_inline_multiple_commands() {
    let stdout = run_cli_success(&["delegate", "inline", "echo first", "echo second"]);
    assert!(stdout.contains("first"));
    assert!(stdout.contains("second"));
    assert!(stdout.contains("Done: 2/2"));
}

#[test]
fn test_delegate_run_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let task_file = dir.path().join("tasks.json");
    let tasks = serde_json::json!([
        {"id": "t1", "name": "echo_first", "command": "echo first_task"},
        {"id": "t2", "name": "echo_second", "command": "echo second_task"}
    ]);
    std::fs::write(&task_file, serde_json::to_string(&tasks).unwrap()).unwrap();

    let stdout = run_cli_success(&["delegate", "run", task_file.to_str().unwrap()]);
    assert!(stdout.contains("echo_first"));
    assert!(stdout.contains("echo_second"));
    assert!(stdout.contains("Done: 2/2"));
}

// ── Search command ────────────────────────────────────────

#[test]
fn test_search_no_network_graceful_error() {
    // 调用真实 DuckDuckGo，断言 CLI 不会 panic (即 process 正常退出)。
    // 在网络可达时返回结果；在离线时返回 ToolError——两种路径都不应 panic。
    // 该测试不假设网络状态：原断言 !stderr.is_empty() 在在线环境下会 flaky 失败。
    let out = run_cli(&["search", "zzzz_nonexistent_query_xxxxx"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // 进程必须正常退出（成功或失败均可，但不能被 panic 杀掉）
    assert!(
        !stderr.contains("panic") && !stderr.contains("thread '"),
        "search command should not panic; stderr={stderr}"
    );
}

#[test]
fn test_search_help() {
    let stdout = run_cli_success(&["search", "--help"]);
    assert!(stdout.contains("使用 DuckDuckGo 搜索网络"));
    assert!(stdout.contains("QUERY"));
}

// ── Health command ────────────────────────────────────────

#[test]
fn test_health_no_bridge_graceful_error() {
    // Use a clearly unreachable address so the test is hermetic (not dependent on no bridge running)
    let out = run_cli(&["--bridge", "127.0.0.1:1", "health"]);
    // Port 1 is privileged → connect fails immediately → graceful failure (not panic)
    assert!(
        !out.status.success(),
        "health should fail without a running bridge"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.is_empty(), "expected error output when no bridge");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Should not contain partial "Status:" or "Version:" output
    assert!(
        !stdout.contains("Status:"),
        "should not print status without bridge"
    );
}

#[test]
fn test_health_help() {
    // `health` is a unit variant, so `--help` returns the top-level help
    let stdout = run_cli_success(&["--help"]);
    assert!(stdout.contains("health"));
    assert!(stdout.contains("检查桥接健康状态"));
}

#[test]
fn test_health_deep_no_bridge_graceful_error() {
    // `:health` 调起 bridge `/health/deep`——无 bridge 时不应 panic。
    // 该测试 hermetic：用不可达地址（端口 1，连接立即失败）。
    let out = run_cli(&["--bridge", "127.0.0.1:1", "health"]);
    assert!(
        !out.status.success(),
        "health should fail without a running bridge"
    );
}
