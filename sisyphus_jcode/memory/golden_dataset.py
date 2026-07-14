import json
import os
import time
from typing import Dict, Any, Optional, Tuple
from ..core.sop_graph import SOPGraphConfig

class GoldenDataset:
    def __init__(self, storage_path: str = "golden_dataset.json"):
        self.storage_path = storage_path
        self.data: Dict[str, Dict[str, Any]] = self._load()

    def _load(self) -> Dict[str, Any]:
        if os.path.exists(self.storage_path):
            with open(self.storage_path, "r") as f:
                return json.load(f)
        return {}

    def _save(self):
        with open(self.storage_path, "w") as f:
            json.dump(self.data, f, indent=2)

    def enroll(self, goal: str, config: SOPGraphConfig, metric: float):
        # Convert SOPGraphConfig to dict for storage
        config_dict = {
            "nodes": [
                {"name": n.name, "kind": n.kind.value, "config": n.config} 
                for n in config.nodes
            ],
            "edges": config.edges,
            "start_node": config.start_node
        }
        
        self.data[goal] = {
            "config": config_dict,
            "metric": metric,
            "timestamp": time.time()
        }
        self._save()

    def verify_no_regression(self, goal: str, current_metric: float) -> bool:
        if goal not in self.data:
            return True # No golden standard, no regression
        
        golden_metric = self.data[goal]["metric"]
        # Regression if current metric is < 90% of golden metric
        return current_metric >= golden_metric * 0.9
