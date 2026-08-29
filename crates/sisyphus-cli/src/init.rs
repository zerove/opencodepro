use anyhow::Result;

/// Run list.
pub fn run_list() -> Result<()> {
    let templates = crate::tools::list_templates();
    println!("Available project templates:");
    for t in &templates {
        println!("  {} — {}", t.name, t.description);
    }
    Ok(())
}

/// Run generate.
pub fn run_generate(template_name: &str, name: &str, dir: &std::path::Path) -> Result<()> {
    let template = crate::tools::TemplateType::from_str_opt(template_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown template '{}'. Use 'sisyphus init list' to see available templates.",
            template_name
        )
    })?;

    let dest = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(dir)
    };

    // 确保目标目录存在
    std::fs::create_dir_all(&dest)?;

    crate::tools::scaffold(template, name, &dest)?;

    let project_path = dest.join(name);
    println!("Created project '{}' at {}", name, project_path.display());
    println!("Template: {}", template.description());

    Ok(())
}
