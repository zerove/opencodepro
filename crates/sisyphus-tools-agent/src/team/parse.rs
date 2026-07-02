use super::{TaskNode, TeamPlan};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Parse an LLM decomposition response into a validated `TeamPlan`.
pub(crate) fn parse_decomposition_response(
    json_str: &str,
    goal_id: &str,
) -> Result<TeamPlan, String> {
    #[derive(Deserialize)]
    struct RawTask {
        #[serde(default)]
        id: String,
        #[serde(default)]
        agent: String,
        #[serde(default)]
        tool_id: String,
        #[serde(default)]
        args: Option<Value>,
        #[serde(default)]
        depends_on: Vec<String>,
    }

    #[derive(Deserialize)]
    struct RawPlan {
        #[serde(default)]
        tasks: Vec<RawTask>,
    }

    let raw: RawPlan = serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse LLM decomposition response as JSON: {e}"))?;

    if raw.tasks.is_empty() {
        return Err("LLM returned empty task list".to_string());
    }

    let mut seen_ids = HashSet::new();
    for task in &raw.tasks {
        if task.id.is_empty() {
            return Err("Task has empty 'id' field".to_string());
        }
        if !seen_ids.insert(task.id.clone()) {
            return Err(format!("Duplicate task ID: '{}'", task.id));
        }
    }

    let mut tasks = Vec::with_capacity(raw.tasks.len());
    for rt in raw.tasks {
        for dep in &rt.depends_on {
            if !seen_ids.contains(dep) {
                return Err(format!(
                    "Task '{}' depends on '{}' which does not exist",
                    rt.id, dep
                ));
            }
        }

        tasks.push(TaskNode {
            id: rt.id,
            agent_id: rt.agent,
            tool_id: rt.tool_id,
            args: rt.args.unwrap_or(Value::Object(serde_json::Map::new())),
            depends_on: rt.depends_on,
        });
    }

    if let Err(cycle) = detect_cycle(&tasks) {
        return Err(format!("Circular dependency detected: {cycle}"));
    }

    Ok(TeamPlan {
        goal_id: goal_id.to_string(),
        tasks,
    })
}

/// Detect a cycle in the task dependency graph.
/// Returns `Err(cycle_description)` if a cycle is found.
pub(crate) fn detect_cycle(tasks: &[TaskNode]) -> Result<(), String> {
    let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
    for task in tasks {
        deps.entry(task.id.as_str()).or_default();
        for dep in &task.depends_on {
            deps.entry(task.id.as_str()).or_default().push(dep.as_str());
        }
    }

    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;

    let mut color: HashMap<&str, u8> = deps.keys().map(|k| (*k, WHITE)).collect();

    fn visit<'a>(
        node: &'a str,
        deps: &HashMap<&'a str, Vec<&'a str>>,
        color: &mut HashMap<&'a str, u8>,
        path: &mut Vec<&'a str>,
    ) -> Result<(), String> {
        let c = color.get(node).copied().unwrap_or(WHITE);
        if c == BLACK {
            return Ok(());
        }
        if c == GRAY {
            let cycle_start = path.iter().position(|n| *n == node).unwrap();
            let cycle: Vec<&str> = path[cycle_start..].to_vec();
            return Err(cycle.join(" → ").clone());
        }

        color.insert(node, GRAY);
        path.push(node);

        if let Some(neighbors) = deps.get(node) {
            for neighbor in neighbors {
                if color.contains_key(neighbor) {
                    visit(neighbor, deps, color, path)?;
                }
            }
        }

        path.pop();
        color.insert(node, BLACK);
        Ok(())
    }

    let nodes: Vec<&str> = deps.keys().copied().collect();
    let mut path = Vec::new();
    for node in nodes {
        visit(node, &deps, &mut color, &mut path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_empty_task_list() {
        let result = parse_decomposition_response(r#"{"tasks":[]}"#, "g1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_parse_single_task() {
        let json = r#"{
            "tasks": [{
                "id": "research",
                "agent": "librarian",
                "tool_id": "librarian/research",
                "args": {"query": "test"},
                "depends_on": []
            }]
        }"#;
        let plan = parse_decomposition_response(json, "g1").unwrap();
        assert_eq!(plan.goal_id, "g1");
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].tool_id, "librarian/research");
        assert_eq!(plan.tasks[0].args, json!({"query": "test"}));
    }

    #[test]
    fn test_parse_duplicate_ids() {
        let json = r#"{
            "tasks": [
                {"id": "t1", "agent": "a", "tool_id": "t", "args": {}, "depends_on": []},
                {"id": "t1", "agent": "b", "tool_id": "t", "args": {}, "depends_on": []}
            ]
        }"#;
        let result = parse_decomposition_response(json, "g1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate"));
    }

    #[test]
    fn test_parse_missing_dep() {
        let json = r#"{
            "tasks": [
                {"id": "t1", "agent": "a", "tool_id": "t", "args": {}, "depends_on": ["nonexistent"]}
            ]
        }"#;
        let result = parse_decomposition_response(json, "g1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn test_parse_circular_dependency() {
        let json = r#"{
            "tasks": [
                {"id": "a", "agent": "x", "tool_id": "t", "args": {}, "depends_on": ["b"]},
                {"id": "b", "agent": "x", "tool_id": "t", "args": {}, "depends_on": ["a"]}
            ]
        }"#;
        let result = parse_decomposition_response(json, "g1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Circular"));
    }

    #[test]
    fn test_parse_diamond_dag() {
        let json = r#"{
            "tasks": [
                {"id": "root", "agent": "m", "tool_id": "t", "args": {}, "depends_on": []},
                {"id": "left", "agent": "m", "tool_id": "t", "args": {}, "depends_on": ["root"]},
                {"id": "right", "agent": "m", "tool_id": "t", "args": {}, "depends_on": ["root"]},
                {"id": "merge", "agent": "m", "tool_id": "t", "args": {}, "depends_on": ["left", "right"]}
            ]
        }"#;
        let plan = parse_decomposition_response(json, "diamond").unwrap();
        assert_eq!(plan.tasks.len(), 4);
        assert_eq!(plan.tasks[3].depends_on, vec!["left", "right"]);
    }

    #[test]
    fn test_parse_empty_id() {
        let json = r#"{
            "tasks": [{"id": "", "agent": "x", "tool_id": "t", "args": {}, "depends_on": []}]
        }"#;
        let result = parse_decomposition_response(json, "g1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty 'id'"));
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_decomposition_response("not json at all", "g1");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_self_dependency() {
        let json = r#"{
            "tasks": [
                {"id": "a", "agent": "x", "tool_id": "t", "args": {}, "depends_on": ["a"]}
            ]
        }"#;
        let result = parse_decomposition_response(json, "g1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Circular"));
    }

    #[test]
    fn test_parse_complex_dag() {
        let json = r#"{
            "tasks": [
                {"id": "research", "agent": "librarian", "tool_id": "librarian/research", "args": {"query": "AI agents 2026"}, "depends_on": []},
                {"id": "code-review", "agent": "momus", "tool_id": "momus/review_code", "args": {"path": "src/lib.rs"}, "depends_on": []},
                {"id": "analyse", "agent": "metis", "tool_id": "metis/analyse", "args": {"request": "synthesize findings"}, "depends_on": ["research", "code-review"]},
                {"id": "report", "agent": "scribe", "tool_id": "scribe/generate_adr", "args": {"title": "Agent team"}, "depends_on": ["analyse"]}
            ]
        }"#;
        let plan = parse_decomposition_response(json, "complex").unwrap();
        assert_eq!(plan.tasks.len(), 4);
        assert_eq!(plan.tasks[0].id, "research");
        assert_eq!(plan.tasks[2].agent_id, "metis");
        assert_eq!(plan.tasks[2].depends_on, vec!["research", "code-review"]);
    }

    #[test]
    fn test_detect_cycle_no_cycle() {
        let tasks = vec![
            TaskNode {
                id: "a".into(),
                agent_id: "x".into(),
                tool_id: "t".into(),
                args: json!({}),
                depends_on: vec![],
            },
            TaskNode {
                id: "b".into(),
                agent_id: "x".into(),
                tool_id: "t".into(),
                args: json!({}),
                depends_on: vec!["a".into()],
            },
        ];
        assert!(detect_cycle(&tasks).is_ok());
    }

    #[test]
    fn test_detect_cycle_simple() {
        let tasks = vec![
            TaskNode {
                id: "a".into(),
                agent_id: "x".into(),
                tool_id: "t".into(),
                args: json!({}),
                depends_on: vec!["b".into()],
            },
            TaskNode {
                id: "b".into(),
                agent_id: "x".into(),
                tool_id: "t".into(),
                args: json!({}),
                depends_on: vec!["a".into()],
            },
        ];
        assert!(detect_cycle(&tasks).is_err());
    }

    #[test]
    fn test_detect_cycle_no_refs() {
        let tasks: Vec<TaskNode> = vec![];
        assert!(detect_cycle(&tasks).is_ok());
    }
}
