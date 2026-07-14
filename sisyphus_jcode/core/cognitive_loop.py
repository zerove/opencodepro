import asyncio
from typing import Any, Dict, Optional, Tuple
from .sop_graph import SOPGraph, SOPNodeConfig, NodeKind
from .executor import JcodeExecutor
from ..memory.golden_dataset import GoldenDataset

class LlmClient:
    """
    Integrated LLM client. 
    For this prototype, it simulates high-fidelity responses.
    """
    def __init__(self, agent_ref=None):
        self.agent_ref = agent_ref

    async def generate(self, prompt: str) -> str:
        p = prompt.lower()
        if "predict" in p:
            return "Expected: success=true, exit_code=0, output='Task completed successfully'"
        
        if "reflect" in p:
            if any(word in p for word in ["fail", "error", "exception", "success=false"]):
                return "PIVOT: The verification failed. Root cause: logic error in implementation. Suggestion: Re-implement with boundary checks."
            return "CONTINUE"
        
        if "architect" in p or "json" in p:
            # Mock a valid SOPGraphConfig JSON for a simple feature implementation
            return """
Reasoning: We need to implement the feature and then verify it.
JSON: {
  "nodes": [
    { "name": "Implement", "kind": "wasm", "config": { "tool_id": "bash_exec", "command": "echo 'Implementing...'" } },
    { "name": "Verify", "kind": "test", "config": { "command": "ls", "workdir": "." } }
  ],
  "edges": [[0, 1]],
  "start_node": 0
}
Plan: Implement then Verify.
"""
        return "Generic LLM response"


class CognitiveLoop:
    def __init__(self, executor: JcodeExecutor, llm: LlmClient, golden_dataset: GoldenDataset):
        self.executor = executor
        self.llm = llm
        self.golden_dataset = golden_dataset
        self.context = {}

    async def predict(self, node_name: str) -> str:
        prompt = f"Predict the output for node {node_name} given context {self.context}"
        return await self.llm.generate(prompt)

    async def reflect(self, node_name: str, prediction: str, actual: str) -> str:
        prompt = f"REFLECT: Node: {node_name}\nPred: {prediction}\nActual: {actual}\nContext: {self.context}\nDecision (CONTINUE/ADJUST/PIVOT):"
        return await self.llm.generate(prompt)

    async def run_sop(self, goal: str, graph: SOPGraph) -> Tuple[str, str]:
        current_node_idx = graph.start_node
        
        while current_node_idx is not None:
            node = graph.get_node(current_node_idx)
            
            # 1. Predict
            prediction = await self.predict(node.name)
            print(f"Prediction for {node.name}: {prediction}")
            
            # 2. Execute
            success, output = await self.executor.execute_node(node.name, node.kind, node.config)
            print(f"Actual output for {node.name}: {output}")
            
            # 3. Reflect
            decision = await self.reflect(node.name, prediction, output)
            print(f"Decision for {node.name}: {decision}")
            
            if "PIVOT" in decision.upper():
                print("PIVOT triggered! Stopping execution for replanning.")
                return "PIVOT", output
            
            if "ADJUST" in decision.upper():
                print("ADJUST triggered! Updating context.")
                # Logic for updating context would go here
            
            # 4. Move to next node
            successors = graph.get_successors(current_node_idx)
            if not successors:
                # Reach end of SOP, check for regression
                metric = 1.0 if success else 0.0
                if not self.golden_dataset.verify_no_regression(goal, current_metric=metric):
                    print("SOP failed regression check!")
                    return "REGRESSION", output
                
                # Enroll as new gold standard if it's a success
                if success:
                    self.golden_dataset.enroll(goal, graph.config, metric)
                    print("Enrolled as new Golden SOP.")
                
                break
            
            current_node_idx = successors[0]
            
        print("SOP execution finished.")
        return "SUCCESS", "SOP completed"
