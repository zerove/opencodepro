use sisyphus_tools_scm::git;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_git_status_behavior() {
    let dir = tempdir().expect("failed to create temp dir");
    let root = dir.path();
    
    git::init(root).expect("git init failed");
    
    let git_args = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git command failed")
    };
    git_args(&["config", "user.email", "test@test.com"]);
    git_args(&["config", "user.name", "Test User"]);

    let file_path = root.join("test.txt");
    fs::write(&file_path, "hello").expect("write failed");
    
    // 1. Untracked
    let s1 = git::status(Some(root)).expect("status failed");
    assert!(!s1.untracked.is_empty());
    
    // 2. Staged
    git::add(&["."], Some(root)).expect("add failed");
    let s2 = git::status(Some(root)).expect("status failed");
    assert!(!s2.staged.is_empty());
    
    // 3. Committed
    git::commit("init", false, Some(root)).expect("commit failed");
    let s3 = git::status(Some(root)).expect("status failed");
    assert!(s3.staged.is_empty());
    assert!(s3.unstaged.is_empty());
    
    // 4. Modified (Unstaged)
    fs::write(&file_path, "hello world").expect("write failed");
    let s4 = git::status(Some(root)).expect("status failed");
    println!("S4 Status: {:?}", s4);
    assert!(!s4.unstaged.is_empty());
}
