# sisyphus-knowledge — Knowledge Management

The knowledge crate provides graph-based storage and retrieval for cognitive assets (tools, SOPs, insights, constraints). It enables the system to sediment learned knowledge and reuse it across sessions.

## Core Types

### KnowledgeGraph

In-memory graph structure that stores assets and their relationships:

```rust
pub struct KnowledgeGraph {
    graph: DiGraph<String, RelationType>,
    node_map: HashMap<String, NodeIndex>,
    assets: HashMap<String, CognitiveAsset>,
}
```

Key operations:
- `add_asset(asset)` — Insert an asset into the graph
- `add_relation(rel)` — Create a directed edge between two assets
- `get_asset(id)` — Retrieve an asset by ID
- `find_assets_by_kind(kind)` — Filter assets by type (Tool, SOP, Insight, Constraint)
- `get_related_assets(id, rel_type)` — Follow edges to find related assets

### AssetManager

Async wrapper around `KnowledgeGraph` with persistence and intelligent retrieval:

```rust
pub struct AssetManager {
    graph: Arc<RwLock<KnowledgeGraph>>,
    storage_path: PathBuf,
}
```

## Features

### Persistence

The knowledge graph can be saved to and loaded from disk as JSON:

```rust
let manager = AssetManager::new(PathBuf::from(".sisyphus/knowledge.json"));

// Save assets
manager.save_asset("asset_1", "File Reader".into(), content, 1.0).await?;

// Persist to disk
manager.save().await?;

// Load from disk (in a new session)
let manager2 = AssetManager::new(PathBuf::from(".sisyphus/knowledge.json"));
manager2.load().await?;
```

### Intelligent Query

Search for assets matching a goal using case-insensitive fuzzy matching:

```rust
// Search across name, description, and content fields
let results = manager.query_tools_for_goal("file reading").await;

// Results are ranked: name matches rank higher than description matches
for asset in results {
    println!("{}: {}", asset.name, asset.description);
}
```

### LLM-Assisted Retrieval

For complex queries, use the LLM to find the best matching asset:

```rust
let best = manager.find_best_asset("read configuration files", llm_client).await?;
if let Some(asset) = best {
    println!("Best match: {}", asset.name);
}
```

## Asset Types

| Kind | Description | Content Type |
|------|-------------|--------------|
| `Tool` | A WASM tool specification | `ToolSpec` |
| `SOP` | A standard operating procedure graph | `SOPGraphConfig` |
| `Insight` | An observation with reasoning | `InsightData` |
| `Constraint` | A rule with strictness level | `ConstraintData` |

## Relationships

Assets can be connected via directed edges:

| Relation | Meaning |
|----------|---------|
| `Solves` | Asset A solves the problem described in Asset B |
| `Requires` | Asset A requires Asset B to function |
| `EvolvedFrom` | Asset A was derived from Asset B |
| `ConflictsWith` | Asset A and Asset B are incompatible |
| `Refines` | Asset A is a more specific version of Asset B |

## Integration

The knowledge crate is used by `sisyphus-core` in:
- **Self-Programming Manager** — Stores learned tools and SOPs
- **Strategic Manager** — Retrieves relevant knowledge for planning
- **Internalization Manager** — Sediments completed work into persistent assets
