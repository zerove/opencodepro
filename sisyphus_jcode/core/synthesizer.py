from typing import List, Tuple, Optional
from .sop_graph import SOPGraph, SOPGraphConfig, SOPNodeConfig, NodeKind

class SOPSynthesizer:
    """
    Generates a Sisyphus SOP graph based on a goal and available tools.
    """
    def __init__(self, llm):
        self.llm = llm

    async def synthesize(self, goal: str, tools: List[str]) -> Tuple[SOPGraph, str]:
        tools_str = "\n".join(tools)
        prompt = f"""
You are a Strategic SOP Architect. Design a high-fidelity SOP graph to achieve the following goal.
Goal: {goal}

Available Node Types:
1. WASM: {{ "tool_id": "...", "fuel_limit": 1000000 }}
2. RETRY: {{ "child": index, "max_retries": 3 }}
3. BRANCH: {{ "branches": {{ "val": idx }}, "default_next": idx }}
4. TEST: {{ "command": "...", "workdir": "..." }}

Available Tools:
{tools_str}

Constraint: Every implementation node (WASM) MUST be followed by a TEST node.

Output Format:
Reasoning: <topology plan>
JSON: <SOPGraphConfig JSON>
Plan: <overall strategy>
"""
        response = await self.llm.generate(prompt)
        
        # Extract JSON from response
        try:
            import json
            json_start = response.find("JSON:") + 5
            json_end = response.rfind("Plan:")
            json_str = response[json_start:json_end].strip()
            data = json.loads(json_str)
            
            # Convert dict to SOPGraphConfig
            nodes = [
                SOPNodeConfig(
                    name=n["name"], 
                    kind=NodeKind(n["kind"]), 
                    config=n["config"]
                ) for n in data["nodes"]
            ]
            edges = [tuple(e) for e in data["edges"]]
            config = SOPGraphConfig(nodes=nodes, edges=edges, start_node=data.get("start_node", 0))
            
            plan_desc = response.split("Plan:")[1].strip() if "Plan:" in response else "Generated SOP"
            return SOPGraph(config), plan_desc
        except Exception as e:
            print(f"SOP Synthesis failed: {e}")
            raise e
