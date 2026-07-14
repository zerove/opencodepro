from dataclasses import dataclass, field
from typing import List, Dict, Any, Optional, Union
from enum import Enum

class NodeKind(Enum):
    WASM = "wasm"
    RETRY = "retry"
    BRANCH = "branch"
    TEST = "test"
    SUBGRAPH = "subgraph"

@dataclass
class SOPNodeConfig:
    name: str
    kind: NodeKind
    config: Dict[str, Any]

@dataclass
class SOPGraphConfig:
    nodes: List[SOPNodeConfig]
    edges: List[tuple[int, int]]
    start_node: int = 0

class SOPGraph:
    def __init__(self, config: SOPGraphConfig):
        self.config = config
        self.nodes = config.nodes
        self.edges = config.edges
        self.start_node = config.start_node

    def get_successors(self, node_idx: int) -> List[int]:
        return [tgt for src, tgt in self.edges if src == node_idx]

    def get_node(self, idx: int) -> SOPNodeConfig:
        return self.nodes[idx]
