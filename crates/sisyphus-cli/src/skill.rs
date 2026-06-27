use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// 列出所有可用技能
pub fn run_list() -> Result<()> {
    let loader = sisyphus_tools::SkillLoader::new()
        .with_user_dir()
        .with_project_dir(Path::new("."));

    let skills = loader.load_all();

    if skills.is_empty() {
        println!("No skills found.");
        println!();
        println!("  Skill directories scanned:");
        if let Some(home) = home_dir() {
            println!("    {}/.config/sisyphus/skills/", home.display());
        }
        println!("    .sisyphus/skills/");
        println!();
        println!("To install a skill:  sisyphus skill install <path-to-SKILL.md>");
        return Ok(());
    }

    println!("Skills found: {}", skills.len());
    println!();

    for skill in &skills {
        let source_tag = match skill.source {
            sisyphus_tools::SkillSource::BuiltIn => "📦",
            sisyphus_tools::SkillSource::User => "👤",
            sisyphus_tools::SkillSource::Project => "🏗",
        };
        println!("  {} {} v{}", source_tag, skill.name, skill.version);
        if !skill.description.is_empty() {
            println!("     {}", skill.description);
        }
        if !skill.triggers.is_empty() {
            let tools: Vec<&str> = skill
                .triggers
                .iter()
                .flat_map(|t| t.tools.iter().map(|s| s.as_str()))
                .collect();
            if !tools.is_empty() {
                println!("     tools: {}", tools.join(", "));
            }
        }
    }

    Ok(())
}

/// 显示某个技能的详细信息
pub fn run_show(name: &str) -> Result<()> {
    let loader = sisyphus_tools::SkillLoader::new()
        .with_user_dir()
        .with_project_dir(Path::new("."));

    let skills = loader.load_all();
    let skill = skills.iter().find(|s| s.name == name);

    match skill {
        Some(s) => {
            println!("{}", s);
            if let Some(ref path) = s.source_path {
                println!("  file: {}", path.display());
            }
            println!();
            println!("--- Instructions ---");
            println!("{}", s.instructions);
        }
        None => {
            anyhow::bail!(
                "Skill '{}' not found. Use 'sisyphus skill list' to see available skills.",
                name
            );
        }
    }

    Ok(())
}

/// 从文件安装技能到用户目录
pub fn run_install(path: &Path) -> Result<()> {
    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    let skill_dir =
        user_skill_dir().context("Cannot determine home directory for skill installation")?;
    std::fs::create_dir_all(&skill_dir)?;

    // 用文件 stem 作为技能名，复制过去
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?;

    let dest = skill_dir.join(format!("{stem}.md"));

    // 如果目标已存在，确认覆盖
    if dest.exists() {
        eprint!("⚠  Skill '{}' already exists. Overwrite? [y/N] ", stem);
        use std::io::Write;
        std::io::stderr().flush().ok();
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf).ok();
        let answer = buf.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            println!("Install cancelled.");
            return Ok(());
        }
    }

    std::fs::copy(path, &dest)?;
    println!("Installed skill '{}' to {}", stem, dest.display());

    Ok(())
}

/// 移除已安装的技能
pub fn run_remove(name: &str) -> Result<()> {
    let skill_dir = user_skill_dir().context("Cannot determine home directory")?;
    let path = skill_dir.join(format!("{name}.md"));

    if !path.exists() {
        anyhow::bail!("Skill '{}' not found at {}", name, path.display());
    }

    std::fs::remove_file(&path)?;
    println!("Removed skill '{}'", name);

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("USERPROFILE").ok().map(PathBuf::from))
}

fn user_skill_dir() -> Option<PathBuf> {
    Some(home_dir()?.join(".config").join("sisyphus").join("skills"))
}
