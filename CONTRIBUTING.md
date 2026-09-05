# 贡献指南

感谢你考虑为 Sisyphus 做贡献！本文档说明本地开发流程、代码规范与提交规范。

## 行为准则

- 尊重所有贡献者；接受建设性反馈
- 关注"为什么"而非"是什么"
- 保持技术讨论与方案论证导向，避免无实质内容的争论

## 快速开始

### 1. 克隆与构建

```bash
git clone https://github.com/zerove/opencodepro.git
cd opencodepro

# 推荐：用 VS Code devcontainer（自动配 nightly Rust + 全部依赖）
# 命令面板 → "Dev Containers: Reopen in Container"
# 详见 .devcontainer/devcontainer.json

# 或本地手动配置（nightly Rust 必需）
rustup toolchain install nightly --component rustfmt clippy rust-src
rustup default nightly

# 预热依赖
cargo fetch

# 验证基础 crate（内存 < 16GB 时排除重型 bridge/sandbox）
cargo check -p sisyphus-types
cargo check -p sisyphus-core
```

### 2. 配置 Pre-commit（可选但强烈推荐）

```bash
# 安装 cargo-deny (供应链审计)
cargo install --locked cargo-deny

# 本项目使用 Makefile + git hooks 替代 pre-commit
# git hooks 自动跑：cargo fmt --all + cargo clippy --workspace -- -D warnings
git config core.hooksPath .git/hooks  # 默认即可
```

CI 与本地验证脚本：

```bash
make ci-quick         # fmt + clippy + 单元测试（约 2 分钟）
make ci-bridge        # 含 bridge 集成测试（约 15 分钟）
make dev              # ci-quick + ci-bridge（完整本地验证）
```

### 3. 跑测试

```bash
# 快速（CI 等价、约 2 分钟）
make ci-quick

# 完整（含 bridge/sandbox，约 15 分钟）
make ci-bridge

# 单 crate 深度测试
cargo test -p sisyphus-core --lib
cargo test -p sisyphus-bridge --test chaos_extended

# Bench
cargo bench -p sisyphus-core
```

## 项目结构

```
crates/
├── sisyphus-core/         # ★ 认知引擎核心（DAG、认知循环）
├── sisyphus-bridge/      # gRPC + REST 桥接服务
├── sisyphus-cli/         # TUI 客户端
├── sisyphus-types/       # 共享类型、Config、traits（无依赖的叶子 crate）
├── sisyphus-intel/       # 情报分析（仿真、反思、演进）
├── sisyphus-mem/         # 持久化（SQLite/Postgres）
├── sisyphus-tools-*/     # 工具子 crate（10 个）
├── sisyphus-roadmap/     # 战略规划
└── ...

book/                     # mdbook 文档源
docs/                     # 技术文档与 ADR
evals/                    # Agent 能力评测
ide/                      # IDE 扩展 + 多语言 SDK
webui/                    # Web 仪表盘
```

详见 `AGENTS.md` 的"项目知识库"段。

## 代码规范

### 强制规则（clippy `-D warnings`）

| 规则 | 示例 | 备注 |
|------|------|------|
| 错误处理用 `thiserror` | `#[derive(thiserror::Error)]` | 禁止 `Result<T, String>` |
| 不在生产代码用 `unwrap()` / `expect()` | 桥接失败需返回 `CoreError` / `TypesError` | 测试 build.rs 允许 |
| 异步用 `tokio` + `async-trait` | 全部 async fn 一致 | |
| 集成测试放 `tests/`，单元测试放 `src/` | `#[cfg(test)] mod tests` | |
| Bench 用 `criterion` | `benches/` 目录 | |
| 桥接测试加 `#[ignore = "requires dedicated runner"]` | CI 跳过 OOM-prone 测试 | |
| 文档测试用 `///` 注释 | 公共 API 加 doctest 示例 | |

### 命名约定

| 类型 | 风格 | 示例 |
|------|------|------|
| Crate 名 | `sisyphus-<role>` | `sisyphus-core`, `sisyphus-bridge` |
| 类型 | PascalCase | `CognitiveReport`, `MySisyphusBridge` |
| 函数/方法 | snake_case | `execute_goal`, `health_deep_assess` |
| 常量 | SCREAMING_SNAKE_CASE | `DEFAULT_FUEL`, `SSE_MAX_CONNECTIONS` |
| 错误变体 | PascalCase（业务术语） | `CoreError::SOPExecutionFailed` |

### 注释与文档

- **中文优先**：本仓库以中文为权威语言（README、book、TODO.md、CHANGELOG.md）
- **代码注释中文**：业务术语、算法解释、TODO 引用都用中文
- **commit message**：英文（保持与 GitHub 默认生态兼容）

## Commit 规范

本项目使用 [Conventional Commits](https://www.conventionalcommits.org/) 风格：

```
<type>(<scope>): <subject>

[optional body]

[optional footer(s)]
```

### Type

| 类型 | 含义 |
|------|------|
| `feat` | 新功能 |
| `fix` | bug 修复 |
| `docs` | 仅文档变更 |
| `test` | 仅测试（生产代码不变） |
| `refactor` | 重构（既非 feat 也非 fix） |
| `chore` | 杂项（CI、依赖、格式化） |
| `style` | 格式调整（不改逻辑） |
| `perf` | 性能优化 |

### Scope（可选）

通常对应 crate 名或子模块：`feat(bridge): ...`、`fix(core): ...`、`docs(TODO): ...`

### Subject

- 中文或英文均可（推荐中文，与本仓库语言一致）
- 不超过 72 字符
- 用祈使句："添加 X" 而非 "添加了 X"
- 首字母不大写
- 末尾不加句号

### 引用 TODO / Issue

在 commit body 末尾引用：

```
feat(bridge): 健康度端点扩展 (TODO 8.2)

- 新增 /health/live (始终 200，livenessProbe)
- 新增 /health/ready (200/503，readinessProbe)
- OpenAPI 文档 + 2 个 chaos_extended 集成测试

Refs: TODO 8.2 第一项
Closes: #123
```

### 常见 commit 模式

- 一个 PR 一个 commit（小型变更）；大型 feature 可多 commit
- 不允许 commit 大段"调试代码"或 `println!`（提交前跑 `cargo fmt` + clippy）
- 不允许 commit 含敏感信息（API key、私钥）

## Pull Request 流程

### 提交前自查清单

- [ ] `cargo fmt --all` 无 diff
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo test -p <changed-crate>` 通过
- [ ] 新功能有单元测试 + 集成测试
- [ ] 公开 API 加 doctest
- [ ] 文档同步（book/、README、TODO.md、CHANGELOG.md）
- [ ] commit message 符合 Conventional Commits

### PR 模板

提交 PR 时 `.github/PULL_REQUEST_TEMPLATE.md` 自动填充，填写：

- **变更摘要**：1-3 行描述做什么
- **测试**：列出新增 / 修改的测试
- **关联 issue / TODO**：引用 `TODO 8.x 第 N 项` 或 `#issue`
- **截图 / 日志**：UI 变更附截图，性能变更附 bench 数据

### Review 流程

1. 自动化 CI（`ci.yml`）必须全绿
2. 至少 1 名 maintainer review 批准
3. squash merge（保留 commit 历史）

## Bug 报告

提交 `.github/ISSUE_TEMPLATE/bug_report.md`，包含：

- 复现步骤
- 期望 vs 实际
- 环境（OS、Rust 版本、bridge / cli 版本）
- 日志（`RUST_LOG=debug` 输出）

## Feature Request

提交 `.github/ISSUE_TEMPLATE/feature_request.md`，描述：

- 问题背景
- 提议方案
- 替代方案
- 影响范围（哪些 crate / API 变化）

## 文档贡献

- `book/` 用 mdbook：`cd book && mdbook serve` 本地预览
- `docs/` 直接编辑 `.md` 文件
- 提交后 CI 触发 mdbook 构建 + kubeconform 校验

## 发布流程

1. 更新 `CHANGELOG.md`：`[Unreleased]` → `[v0.x.x] - <日期>`
2. 更新 `book/src/configuration.md` 的版本号引用
3. 打 git tag：`git tag -a v0.x.x -m "..."`
4. 推送 tag 触发 `.github/workflows/release.yml`：跨平台构建 + GitHub Release 自动生成
5. Docker 镜像：推送 tag 自动触发 `.github/workflows/docker-release.yml`
   - 多平台构建（linux/amd64 + linux/arm64）
   - 推送至 `ghcr.io/zerove/sisyphus-bridge:v0.x.x`
   - 验证：`docker pull ghcr.io/zerove/sisyphus-bridge:v0.x.x`

## 联系方式

- GitHub Issues: 优先
- Discussions: 设计讨论、长期规划

## 许可证

贡献者协议：提交 PR 即表示同意按 [MIT 协议](./LICENSE) 授权你的贡献。

---

**再次感谢你的贡献！** 🚀