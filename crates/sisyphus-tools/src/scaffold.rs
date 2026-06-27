use crate::ToolResult;
use std::path::Path;

/// Supported project template types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateType {
    RustLib,
    RustBin,
    RustWorkspace,
}

impl TemplateType {
    /// Parse from a string (case-insensitive).
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "rust-lib" | "lib" => Some(Self::RustLib),
            "rust-bin" | "bin" => Some(Self::RustBin),
            "rust-workspace" | "workspace" => Some(Self::RustWorkspace),
            _ => None,
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::RustLib => "Rust library crate with Cargo.toml + src/lib.rs + test",
            Self::RustBin => "Rust binary crate with Cargo.toml + src/main.rs",
            Self::RustWorkspace => "Rust workspace with core/ + cli/ member crates",
        }
    }

    fn directory_name(&self) -> &'static str {
        match self {
            Self::RustLib => "rust-lib",
            Self::RustBin => "rust-bin",
            Self::RustWorkspace => "rust-workspace",
        }
    }
}

/// Metadata about an available template.
#[derive(Debug, Clone)]
pub struct TemplateInfo {
    pub name: &'static str,
    pub description: &'static str,
}

/// List all available scaffold templates.
pub fn list_templates() -> Vec<TemplateInfo> {
    vec![
        TemplateInfo {
            name: TemplateType::RustLib.directory_name(),
            description: TemplateType::RustLib.description(),
        },
        TemplateInfo {
            name: TemplateType::RustBin.directory_name(),
            description: TemplateType::RustBin.description(),
        },
        TemplateInfo {
            name: TemplateType::RustWorkspace.directory_name(),
            description: TemplateType::RustWorkspace.description(),
        },
    ]
}

/// Generate a project from a template into `dest`.
///
/// `dest` should be the parent directory; a subdirectory named `name` will be created.
pub fn scaffold(template: TemplateType, name: &str, dest: &Path) -> ToolResult<()> {
    let project_dir = dest.join(name);

    if project_dir.exists() {
        return Err(crate::ToolError::Generic(format!(
            "Target directory '{}' already exists",
            project_dir.display()
        )));
    }

    match template {
        TemplateType::RustLib => scaffold_rust_lib(&project_dir, name),
        TemplateType::RustBin => scaffold_rust_bin(&project_dir, name),
        TemplateType::RustWorkspace => scaffold_rust_workspace(&project_dir, name),
    }
}

// ── Rust Library ────────────────────────────────────────────

fn scaffold_rust_lib(project_dir: &Path, name: &str) -> ToolResult<()> {
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir)?;

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
"#
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml.as_bytes())?;

    let lib_rs = r#"pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
"#;
    std::fs::write(src_dir.join("lib.rs"), lib_rs.as_bytes())?;

    Ok(())
}

// ── Rust Binary ─────────────────────────────────────────────

fn scaffold_rust_bin(project_dir: &Path, name: &str) -> ToolResult<()> {
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir)?;

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
"#
    );
    std::fs::write(project_dir.join("Cargo.toml"), cargo_toml.as_bytes())?;

    let main_rs = r#"fn main() {
    println!("Hello, world!");
}
"#;
    std::fs::write(src_dir.join("main.rs"), main_rs.as_bytes())?;

    Ok(())
}

// ── Rust Workspace ──────────────────────────────────────────

fn scaffold_rust_workspace(project_dir: &Path, name: &str) -> ToolResult<()> {
    // Workspace root Cargo.toml
    let workspace_toml = format!(
        r#"[workspace]
members = [
    "crates/core",
    "crates/cli",
]
resolver = "2"

[workspace.dependencies]

[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
"#
    );
    std::fs::create_dir_all(project_dir.join("crates").join("core").join("src"))?;
    std::fs::create_dir_all(project_dir.join("crates").join("cli").join("src"))?;

    std::fs::write(project_dir.join("Cargo.toml"), workspace_toml.as_bytes())?;

    // core crate
    let core_toml = format!(
        r#"[package]
name = "{name}-core"
version = "0.1.0"
edition = "2021"

[dependencies]
"#
    );
    std::fs::write(
        project_dir.join("crates").join("core").join("Cargo.toml"),
        core_toml.as_bytes(),
    )?;

    let core_lib = r#"pub fn core_function() -> &'static str {
    "hello from core"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core() {
        assert_eq!(core_function(), "hello from core");
    }
}
"#;
    std::fs::write(
        project_dir
            .join("crates")
            .join("core")
            .join("src")
            .join("lib.rs"),
        core_lib.as_bytes(),
    )?;

    // cli crate
    let cli_toml = format!(
        r#"[package]
name = "{name}-cli"
version = "0.1.0"
edition = "2021"

[dependencies]
{name}-core = {{ path = "../core" }}
"#
    );
    std::fs::write(
        project_dir.join("crates").join("cli").join("Cargo.toml"),
        cli_toml.as_bytes(),
    )?;

    let cli_main = format!(
        r#"fn main() {{
    println!("{{}}", {name}_core::core_function());
}}
"#
    );
    std::fs::write(
        project_dir
            .join("crates")
            .join("cli")
            .join("src")
            .join("main.rs"),
        cli_main.as_bytes(),
    )?;

    Ok(())
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_template_type_from_str() {
        assert_eq!(
            TemplateType::from_str_opt("rust-lib"),
            Some(TemplateType::RustLib)
        );
        assert_eq!(
            TemplateType::from_str_opt("lib"),
            Some(TemplateType::RustLib)
        );
        assert_eq!(
            TemplateType::from_str_opt("RUST-LIB"),
            Some(TemplateType::RustLib)
        );
        assert_eq!(
            TemplateType::from_str_opt("rust-bin"),
            Some(TemplateType::RustBin)
        );
        assert_eq!(
            TemplateType::from_str_opt("bin"),
            Some(TemplateType::RustBin)
        );
        assert_eq!(
            TemplateType::from_str_opt("workspace"),
            Some(TemplateType::RustWorkspace)
        );
        assert_eq!(TemplateType::from_str_opt("unknown"), None);
    }

    #[test]
    fn test_list_templates() {
        let templates = list_templates();
        assert_eq!(templates.len(), 3);
        assert!(templates.iter().any(|t| t.name == "rust-lib"));
        assert!(templates.iter().any(|t| t.name == "rust-bin"));
        assert!(templates.iter().any(|t| t.name == "rust-workspace"));
    }

    #[test]
    fn test_scaffold_rust_lib() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("generated");
        std::fs::create_dir_all(&dest).unwrap();

        scaffold(TemplateType::RustLib, "my-lib", &dest).unwrap();

        let project = dest.join("my-lib");
        assert!(project.join("Cargo.toml").exists());
        assert!(project.join("src").join("lib.rs").exists());

        let cargo = std::fs::read_to_string(project.join("Cargo.toml")).unwrap();
        assert!(cargo.contains(r#"name = "my-lib""#));
        assert!(cargo.contains("edition = \"2021\""));

        let lib = std::fs::read_to_string(project.join("src").join("lib.rs")).unwrap();
        assert!(lib.contains("pub fn add"));
        assert!(lib.contains("#[test]"));
    }

    #[test]
    fn test_scaffold_rust_bin() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("generated");
        std::fs::create_dir_all(&dest).unwrap();

        scaffold(TemplateType::RustBin, "my-bin", &dest).unwrap();

        let project = dest.join("my-bin");
        assert!(project.join("Cargo.toml").exists());
        assert!(project.join("src").join("main.rs").exists());

        let main = std::fs::read_to_string(project.join("src").join("main.rs")).unwrap();
        assert!(main.contains("fn main()"));
        assert!(main.contains("Hello, world!"));
    }

    #[test]
    fn test_scaffold_rust_workspace() {
        let dir = TempDir::new().unwrap();
        let dest = dir.path().join("generated");
        std::fs::create_dir_all(&dest).unwrap();

        scaffold(TemplateType::RustWorkspace, "my-ws", &dest).unwrap();

        let project = dest.join("my-ws");
        // Root Cargo.toml should be workspace manifest
        let root_toml = std::fs::read_to_string(project.join("Cargo.toml")).unwrap();
        assert!(root_toml.contains("[workspace]"));
        assert!(root_toml.contains(r#""crates/core""#));
        assert!(root_toml.contains(r#""crates/cli""#));

        // core crate
        assert!(project
            .join("crates")
            .join("core")
            .join("Cargo.toml")
            .exists());
        assert!(project
            .join("crates")
            .join("core")
            .join("src")
            .join("lib.rs")
            .exists());
        let core_lib = std::fs::read_to_string(
            project
                .join("crates")
                .join("core")
                .join("src")
                .join("lib.rs"),
        )
        .unwrap();
        assert!(core_lib.contains("core_function"));

        // cli crate
        assert!(project
            .join("crates")
            .join("cli")
            .join("Cargo.toml")
            .exists());
        assert!(project
            .join("crates")
            .join("cli")
            .join("src")
            .join("main.rs")
            .exists());
        let cli_toml =
            std::fs::read_to_string(project.join("crates").join("cli").join("Cargo.toml")).unwrap();
        assert!(cli_toml.contains("my-ws-core"));
    }

    #[test]
    fn test_scaffold_existing_dir_returns_error() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("existing")).unwrap();

        let result = scaffold(TemplateType::RustLib, "existing", dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }
}
