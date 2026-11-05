# 部署

Sisyphus 支持多种部署方式：从源码 `cargo run`、预编译二进制、Docker 镜像、Kubernetes manifests。

## Docker 镜像

官方镜像发布到 [GitHub Container Registry](https://github.com/zerove/opencodepro/pkgs/container/sisyphus-bridge)：

```bash
# 拉取最新稳定版（v0.2.0+ 后）
docker pull ghcr.io/zerove/sisyphus-bridge:latest

# 拉取特定版本
docker pull ghcr.io/zerove/sisyphus-bridge:v0.2.0

# 多平台：linux/amd64 + linux/arm64（Apple Silicon / Raspberry Pi / Graviton）
```

镜像基于 `Dockerfile`（multi-stage）：
- Builder: `rust:1.99.1-slim-bookworm`（宿主）+ `rustup` 安装 `rust-toolchain.toml`
  pin 住的工具链（Docker 官方 rust 镜像已不再发布 nightly 标签）
- Runtime: `debian:bookworm-slim`
- 暴露端口：50051 (gRPC) + 8080 (HTTP/REST)
- 数据卷：`/app/data`（持久化 SQLite/PostgreSQL 数据）

### 运行

```bash
# 直接启动
docker run -d \
  --name sisyphus-bridge \
  -p 50051:50051 \
  -p 8080:8080 \
  -e SISYPHUS_LLM_API_KEY="your-api-key" \
  -e SISYPHUS_LLM_ENDPOINT="https://api.openai.com/v1/chat/completions" \
  -v sisyphus-data:/app/data \
  ghcr.io/zerove/sisyphus-bridge:latest

# 用 docker-compose（含 PostgreSQL profile）
docker compose --profile db up -d
```

### 自动发布

`.github/workflows/docker-release.yml` 在推送 `v*` tag 时自动：

1. 多平台构建（linux/amd64 + linux/arm64 via QEMU）
2. 推送至 ghcr.io，tags：
   - `v0.2.0` → `:v0.2.0`, `:0.2`, `:0`, `:latest`
   - `v0.2.0-rc.1` → `:v0.2.0-rc.1`（预发布标记）
3. 缓存层用 GitHub Actions cache（`type=gha,mode=max`）减少重复构建时间

## Kubernetes

仓库根目录 `k8s/` 提供 manifests（systemd unit 同样）：

```bash
# 部署到默认 namespace
kubectl apply -f k8s/bridge-deployment.yaml
kubectl apply -f k8s/bridge-service.yaml

# 查看状态
kubectl get pods -l app=sisyphus-bridge
kubectl logs -f deployment/sisyphus-bridge
```

详细字段见各 manifest 顶部注释。

## 从源码

适合开发或自定义构建：

```bash
# 本地运行
cargo run -p sisyphus-bridge

# 自定义 Docker 构建
docker build -t my-sisyphus:latest .
```

## 健康度端点

部署后用以下端点验证：

- `GET /health/live` — 200（存活）
- `GET /health/ready` — 200/503（依赖就绪）
- `GET /health/deep` — 200/207（组件级状态）
- `GET /api/metrics` — Prometheus 文本

详见 [REST API](./rest-api.md) 与 [K8s 集成示例](./tutorial.md#示例-2bridge-场景--rest-api-提交目标)。

## 升级路径

v0.2.0 → v0.3.0 升级时注意：

1. **Trait 变更**：`ExperienceBank` 新增 `export_version_vector()`；若你有本仓库外的
   实现，需补该方法（仓库内实现均已更新）
2. **配置 schema**：无变化，`v0.2.0` 的 `config.toml` 可直接使用
3. **依赖镜像 tag 升级**：`docker pull ghcr.io/zerove/sisyphus-bridge:v0.3.0` 后重启
4. **经验主键**：无需迁移。`v0.2.0` 之前的旧格式主键（`exp:{timestamp}`）仍可读，
   只是不参与序号恢复

v0.1.0 → v0.2.0 升级时注意：

1. **配置 schema 变化**：见 `CHANGELOG.md [v0.2.0]` 段（如有破坏性变更）
2. **数据迁移**：SQLite → PostgreSQL 用 `scripts/migrate.sh`（待实现）
3. **依赖镜像 tag 升级**：`docker pull ghcr.io/zerove/sisyphus-bridge:v0.2.0` 后重启
4. **健康度端点**：K8s `livenessProbe` / `readinessProbe` 配置更新（若 path 改变）