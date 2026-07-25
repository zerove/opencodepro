use sisyphus_tools_scm::{git, patch};
use std::fs;

use tempfile::tempdir;

#[test]
fn test_scm_integration_workflow() {
    // 1. Setup: Create a temporary directory and init git repo
    let dir = tempdir().expect("failed to create temp dir");
    let root = dir.path();

    git::init(root).expect("git init failed");

    // Set git config for the temp repo
    let git_args = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git command failed")
    };
    git_args(&["config", "user.email", "test@test.com"]);
    git_args(&["config", "user.name", "Test User"]);

    // 2. Initial Commit
    let file_path = root.join("main.rs");
    fs::write(&file_path, "fn main() {\n    println!(\"hello\");\n}\n")
        .expect("failed to write file");

    git::add(&["."], Some(root)).expect("git add failed");
    git::commit("initial commit", false, Some(root)).expect("git commit failed");

    let s1 = git::status(Some(root)).expect("git status failed");
    assert!(s1.staged.is_empty());
    assert!(s1.untracked.is_empty());

    // 3. Branching and Modification
    git::checkout("feature", true, Some(root)).expect("git checkout -b failed");

    let new_content = "fn main() {\n    println!(\"hello world\");\n}\n";
    fs::write(&file_path, new_content).expect("failed to write updated file");

    let diff = git::diff(None, false, Some(root)).expect("git diff failed");
    assert!(diff.contains("+    println!(\"hello world\");"));

    git::add(&["."], Some(root)).expect("git add failed");
    git::commit("updated hello", false, Some(root)).expect("git commit failed");

    // 4. Patching
    // Create a patch to add a new line
    let patch_content = "\
--- a/main.rs
+++ b/main.rs
@@ -1,3 +1,4 @@
 fn main() {
     println!(\"hello world\");
+    println!(\"patch applied\");
 }
";
    patch::apply_patch(&file_path, patch_content).expect("apply_patch failed");

    let final_content = fs::read_to_string(&file_path).expect("failed to read file");
    assert!(final_content.contains("patch applied"));

    // 5. Final Status
    let s2 = git::status(Some(root)).expect("git status failed");
    println!("Final Git Status: {:?}", s2);
    assert!(
        !s2.unstaged.is_empty(),
        "Expected unstaged changes after patch, but found none. Status: {:?}",
        s2
    );
}
