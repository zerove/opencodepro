use sisyphus_tools_system::{bash, file};
use tempfile::tempdir;

#[tokio::test]
async fn test_system_integration_workflow() {
    // 1. Setup: Create a temporary workspace
    let dir = tempdir().expect("failed to create temp dir");
    let root = dir.path();

    let file1_path = root.join("hello.txt");
    let file2_path = root.join("world.txt");
    let sub_dir = root.join("src");
    let file3_path = sub_dir.join("main.rs");

    // 2. File Operations: Write files
    file::write(&file1_path, "Hello Sisyphus!\n").expect("failed to write file1");
    file::write(&file2_path, "Welcome to the cognitive engine.\n").expect("failed to write file2");
    file::write(&file3_path, "fn main() {\n    println!(\"hello\");\n}\n")
        .expect("failed to write file3");

    // 3. Explore: Globbing
    let txt_files = file::glob("*.txt", root).expect("glob failed");
    assert_eq!(txt_files.len(), 2);
    assert!(txt_files.iter().any(|p| p.ends_with("hello.txt")));
    assert!(txt_files.iter().any(|p| p.ends_with("world.txt")));

    // 4. Explore: Grep
    let matches = file::grep("Sisyphus", root, None).expect("grep failed");
    assert_eq!(matches.len(), 1);
    assert!(matches[0].content.contains("Hello Sisyphus!"));

    // 5. File Operations: Edit
    file::edit(&file1_path, "Sisyphus", "Jcode").expect("edit failed");
    let content = file::read(&file1_path).expect("read failed");
    assert_eq!(content, "Hello Jcode!\n");

    // 6. Bash: Execute command in workdir
    let bash_result =
        bash::execute("cat hello.txt", Some(root), 5000).await.expect("bash execute failed");
    assert_eq!(bash_result.stdout.trim(), "Hello Jcode!");

    // 7. Bash: Complex command (piping/wc)
    let wc_result =
        bash::execute("ls -1 *.txt | wc -l", Some(root), 5000).await.expect("bash wc failed");
    assert_eq!(wc_result.stdout.trim(), "2");
}

#[tokio::test]
async fn test_system_error_handling() {
    let dir = tempdir().expect("failed to create temp dir");
    let root = dir.path();

    // Test: Read non-existent file
    let res = file::read(&root.join("ghost.txt"));
    assert!(res.is_err());

    // Test: Bash timeout
    let res_timeout = bash::execute("sleep 2", Some(root), 100).await;
    assert!(res_timeout.is_err());
}
