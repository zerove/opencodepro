<!--
Sisyphus Pull Request 模板。
填写后保留不相关的小节用 `<!-- N/A -->`。
-->

## 变更摘要

<!-- 1-3 行描述本次 PR 做什么 -->

## 关联 Issue / TODO

<!-- 例：TODO 8.4 第一项 / Closes #123 -->

Refs: <!-- TODO X.Y 第 N 项 / Issue # -->

## 变更类型

<!-- 勾选适用的 -->

- [ ] 新功能（feature）
- [ ] bug 修复
- [ ] 重构（无功能变化）
- [ ] 性能优化（附 bench 数据）
- [ ] 文档变更
- [ ] 仅测试
- [ ] CI / 构建系统

## 测试覆盖

<!-- 列出新增 / 修改的测试 -->

- [ ] 单元测试：<!-- 哪些函数 -->
- [ ] 集成测试：<!-- 哪些 test 路径 -->
- [ ] 文档测试（doctest）：
- [ ] 手工验证：<!-- 跑了什么命令 / 看到了什么输出 -->

## 影响范围

<!-- 列出受影响的 crate、公开 API、配置项 -->

- 受影响 crate：<!-- e.g. sisyphus-bridge -->
- 公开 API 变化：<!-- breaking change 必须说明 -->
- 配置 / 环境变量：<!-- e.g. 新增 SISYPHUS_* env var -->

## 文档同步

- [ ] book/ 章节（如有）
- [ ] CHANGELOG.md Unreleased 段
- [ ] TODO.md 状态更新
- [ ] README.md（如有）

## 自查清单

提交前请确认：

- [ ] `cargo fmt --all` 无 diff
- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo test -p <changed-crate>` 通过
- [ ] commit message 符合 [Conventional Commits](./CONTRIBUTING.md#commit-规范)
- [ ] 没有敏感信息（API key / 私钥 / 凭据）

## 截图 / 日志

<!-- UI 变更附截图；性能变更附 criterion 输出；错误修复附 before/after 日志 -->

```text
（粘贴输出）
```

## 备注

<!-- Reviewer 注意事项、阻塞点、TODO 引用等 -->