with open("crates/sisyphus-types/src/graph.rs", "r") as f:
    lines = f.readlines()

start_line = -1
for i in range(len(lines)):
    if "let action = match cfg.kind {" in lines[i]:
        start_line = i
        break

if start_line != -1:
    end_line = start_line
    brace_count = 0
    while end_line < len(lines):
        brace_count += lines[end_line].count('{')
        brace_count -= lines[end_line].count('}')
        if brace_count == 0:
            break
        end_line += 1
    
    replacement = [
        "            let action = match cfg.kind {\n",
        "                NodeKind::Wasm { tool_id, .. } => crate::execution::CognitiveAction::ToolCall { tool_id, args: serde_json::json!({}) },\n",
        "                NodeKind::Retry { .. } => crate::execution::CognitiveAction::ToolCall { tool_id: \"retry_wrapper\".into(), args: serde_json::json!({}) },\n",
        "                NodeKind::Branch { .. } => crate::execution::CognitiveAction::ToolCall { tool_id: \"branch_logic\".into(), args: serde_json::json!({}) },\n",
        "                NodeKind::SubGraph { .. } => crate::execution::CognitiveAction::ToolCall { tool_id: \"subgraph_exec\".into(), args: serde_json::json!({}) },\n",
        "                NodeKind::Test { .. } => crate::execution::CognitiveAction::ToolCall { tool_id: \"test_exec\".into(), args: serde_json::json!({}) },\n",
        "                _ => crate::execution::CognitiveAction::ToolCall { tool_id: \"internal_handler\".into(), args: serde_json::json!({}) },\n",
        "            };\n"
    ]
    
    lines[start_line:end_line + 1] = replacement
    
    with open("crates/sisyphus-types/src/graph.rs", "w") as f:
        f.writelines(lines)
    print("Successfully updated match block in graph.rs")
else:
    print("Could not find the match block in graph.rs")
