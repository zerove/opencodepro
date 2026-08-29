use anyhow::Result;

use crate::commands::*;

/// Handle plan.
pub fn handle_plan(sub: &PlanCommands) -> Result<()> {
    match sub {
        PlanCommands::Create {
            name,
            description,
            file,
        } => {
            let reqs = match file {
                Some(p) => std::fs::read_to_string(p)?,
                None => description.clone(),
            };
            let plan = sisyphus_cli::tools::plan_from_requirements(name, &reqs);
            println!("Plan: {} ({} steps)", plan.name, plan.steps.len());
            println!("Status: {}", plan.status);
            for step in &plan.steps {
                println!("  {} — {}", step.id, step.description);
            }
        }
        PlanCommands::Save { plan_file, output } => {
            let plan = sisyphus_cli::tools::plan_load(plan_file)?;
            sisyphus_cli::tools::plan_save(&plan, output)?;
            println!("Saved plan to {}", output.display());
        }
        PlanCommands::Show { plan_file } => {
            let plan = sisyphus_cli::tools::plan_load(plan_file)?;
            println!("Plan: {} — {} ({})", plan.name, plan.status, plan.id);
            println!("{}", plan.description);
            for step in &plan.steps {
                println!("  {} — {} [{}]", step.id, step.description, step.status);
            }
        }
        PlanCommands::Review { plan_file } => {
            let mut plan = sisyphus_cli::tools::plan_load(plan_file)?;
            let issues = sisyphus_cli::tools::plan_review(&plan);
            if issues.is_empty() {
                println!("No issues found.");
                sisyphus_cli::tools::plan_approve(&mut plan);
            } else {
                for issue in &issues {
                    println!("  ⚠ {issue}");
                }
            }
        }
        PlanCommands::Approve { plan_file } => {
            let mut plan = sisyphus_cli::tools::plan_load(plan_file)?;
            sisyphus_cli::tools::plan_approve(&mut plan);
            sisyphus_cli::tools::plan_save(&plan, plan_file)?;
            println!("Approved plan: {}", plan.name);
        }
    }
    Ok(())
}
