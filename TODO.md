# Sisyphus 后续开发计划 (TODO.md)

## 阶段 1: 基础设施维护与代码卫生 (Maintenance & Hygiene)
- [ ] **清除重构残留**: 修复 `cargo check` 报告的所有未使用导入和变量 (High)
- [ ] **死代码清理**: 移除未使用的结构体、方法和常量 (Medium)
- [ ] **错误处理审计**: 确保所有生产代码遵循 `AGENTS.md` 规范，严禁 `unwrap()` (High)
- [ ] **文档同步**: 更新 `book/` 和 `docs/` 以反映最新的 crate 拆分架构 (Medium)

## 阶段 2: 功能扩展 (Feature Expansion)
- [ ] **REST API 接口层**: 
    - [ ] 选择框架 (Axum/Actix) 并搭建基础服务 (High)
    - [ ] 将 gRPC `ExecuteGoal`/`ExecuteSop` 映射为 HTTP 接口 (High)
    - [ ] 实现 OpenAPI/Swagger 文档自动生成 (Medium)
- [ ] **多工作区支持 (Multi-Workspace)**:
    - [ ] 扩展 `RoadmapManager` 支持多项目 ID 隔离 (High)
    - [ ] 在 `MfsStore` 中实现工作区级路径隔离 (High)
    - [ ] 更新 TUI CLI 支持工作区切换 (Medium)
- [ ] **技能市场 (Skill Market)**:
    - [ ] 设计动态技能加载协议 (Medium)
    - [ ] 实现从远程/本地仓库安装新工具的能力 (Medium)

## 阶段 3: 认知能力深化 (Core Intelligence Enhancement)
- [ ] **概率引导规划**: 将蒙特卡洛模拟的路径概率集成到 `Predict` 阶段，实现概率最优路径选择 (High)
- [ ] **高级 SOP 演进**: 基于失败模式库实现更智能的图突变 (SOP Mutation) (Medium)
- [ ] **深度反思机制**: 优化偏差评分算法，提供更具体的认知修正建议 (Medium)

---
**更新日期**: 2026-07-14
**当前重点**: 阶段 1 (代码卫生) $\to$ 阶段 2 (REST API)
