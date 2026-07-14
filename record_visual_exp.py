with open("crates/sisyphus-core/src/lib.rs", "r") as f:
    lines = f.readlines()

# Find the block that starts with "// Record structured experience for Monte Carlo biasing"
start_idx = -1
for i in range(len(lines)):
    if "// Record structured experience for Monte Carlo biasing" in lines[i]:
        start_idx = i
        break

if start_idx != -1:
    # Find where the block ends (the record call)
    end_idx = start_idx
    while end_idx < len(lines):
        if "self.experience_bank.record(experience).await" in lines[end_idx]:
            end_idx = end_idx + 1
            break
        end_idx += 1
    
    # New replacement code
    replacement = [
        "        // Record structured experience for Monte Carlo biasing\n",
        "        let mut experience = sisyphus_types::experience::ExperienceTuple {\n",
        "            context_text: goal.to_string(),\n",
        "            context_hash: format!(\"{:x}\", md5::compute(goal)),\n",
        "            action_id: sop_id.to_string(),\n",
        "            outcome_success: success,\n",
        "            deviation_score: deviation.unwrap_or(0.0),\n",
        "            cost: 0, // Actual cost tracking TBD\n",
        "            timestamp: std::time::SystemTime::now()\n",
        "                .duration_since(std::time::UNIX_EPOCH)\n",
        "                .unwrap_or_default()\n",
        "                .as_secs(),\n",
        "            embedding: None,\n",
        "            visual_embedding: None,\n",
        "            insight: None,\n",
        "        };\n",
        "\n",
        "        // Capture visual experience if successful\n",
        "        if success {\n",
        "            if let Some(ref v_manager) = self.strategic_manager.visual_manager {\n",
        "                // In a real system, we would capture the current screenshot.\n",
        "                // For now, we simulate the visual state.\n",
        "                let dummy_image = vec![0u8; 1024];\n",
        "                if let Ok(vis_emb) = v_manager.get_embedding(&dummy_image).await {\n",
        "                    experience.visual_embedding = Some(vis_emb);\n",
        "                }\n",
        "            }\n",
        "        }\n",
        "\n",
        "        let _ = self.experience_bank.record(experience).await;\n"
    ]
    
    # Replace the lines
    lines[start_idx:end_idx] = replacement
    
    with open("crates/sisyphus-core/src/lib.rs", "w") as f:
        f.writelines(lines)
    print("Successfully updated execute_and_record for visual experience recording")
else:
    print("Could not find the experience recording block")
