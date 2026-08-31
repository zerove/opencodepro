use anyhow::Result;

use crate::commands::GithubCommands;

pub(crate) fn handle_github(sub: &GithubCommands) -> Result<()> {
    match sub {
        GithubCommands::Action {
            kind,
            output,
            toolchain,
            cargo_flags: _,
            exclude,
            clippy,
            deny,
        } => {
            let wk = match kind.as_str() {
                "ci" => sisyphus_cli::tools::WorkflowKind::Ci,
                "pr-review" => sisyphus_cli::tools::WorkflowKind::PrReview,
                "release" => sisyphus_cli::tools::WorkflowKind::Release,
                other => {
                    anyhow::bail!("Unknown workflow kind: {other}. Use: ci, pr-review, release")
                },
            };
            let opts = sisyphus_cli::tools::GenerateOptions {
                rust_toolchain: toolchain.clone(),
                cargo_extra_flags: String::new(),
                exclude_packages: exclude.clone(),
                include_clippy: *clippy,
                include_deny: *deny,
            };
            let path = match output {
                Some(p) => {
                    let content = sisyphus_cli::tools::generate_workflow(wk, &opts);
                    std::fs::write(p, content)?;
                    println!("Workflow written to {}", p.display());
                    p.clone()
                },
                None => sisyphus_cli::tools::write_workflow(wk, &opts)?,
            };
            println!("Generated {} workflow at {}", wk.as_str(), path.display());
        },
        GithubCommands::CiConfig => {
            let cfg = sisyphus_cli::tools::CiConfig::from_env();
            println!("CI mode: {}", cfg.ci_mode);
            println!("Output format: {}", cfg.output_format);
            match &cfg.github_token {
                Some(_) => println!("GitHub token: present"),
                None => println!("GitHub token: not set"),
            }
            match &cfg.pr_number {
                Some(n) => println!("PR number: {n}"),
                None => println!("PR number: not set"),
            }
        },
    }
    Ok(())
}
