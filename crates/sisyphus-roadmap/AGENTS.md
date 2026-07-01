# Sisyphus Roadmap Agent Knowledge Base

## OVERVIEW
Strategic planning and governance: RoadmapManager, milestone tracking, autonomous goal generation, resource optimization.

## WHERE TO LOOK
- Core manager: `roadmap_manager.rs` (project CRUD, milestone lifecycle)
- Governance loop: `tests/governance_loop.rs` (strategic reflection, autonomous milestones)
- Data model: inline in lib.rs (Project, Milestone, Phase, Goal types)

## CONVENTIONS
- MfsStore-backed persistence (same L1/L2 pattern as sisyphus-mem)
- Governance loop runs on configurable interval
- Goal generation uses LLM via cognitive engine bridge
- All mutations go through RoadmapManager (enforces invariants)

## COMMANDS
```bash
cargo test -p sisyphus-roadmap       # Includes governance_loop integration test
```
