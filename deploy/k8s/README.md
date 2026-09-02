# ── Sisyphus Bridge 部署到 Kubernetes ───────────────────────────────
# 部署步骤：
#   1. 构建并推送镜像：
#      docker build -t ghcr.io/zerove/sisyphus-bridge:v0.x.0 .
#      docker push ghcr.io/zerove/sisyphus-bridge:v0.x.0
#   2. 修改 03-deployment.yaml 中 image 标签为 v0.x.0
#   3. 应用 manifests：
#      kubectl apply -f deploy/k8s/
#   4. 验证：
#      kubectl -n sisyphus get pods -l app.kubernetes.io/name=sisyphus-bridge
#      kubectl -n sisyphus logs -l app.kubernetes.io/name=sisyphus-bridge --tail=50
#   5. 端口转发验证 HTTP：
#      kubectl -n sisyphus port-forward svc/sisyphus-bridge-http 8080:8080
#      curl http://127.0.0.1:8080/health/deep
#
# 卸载：
#   kubectl delete -f deploy/k8s/
#
# 资源清单：
# - 00-namespace.yaml    命名空间
# - 01-configmap.yaml    配置
# - 02-secret.yaml       敏感凭据（占位，需替换为 sealed-secrets）
# - 03-deployment.yaml   Deployment（2 副本 + 滚动升级 + 健康探针）
# - 04-service.yaml      Service（gRPC + HTTP ClusterIP）
# - 05-pdb.yaml          PodDisruptionBudget（至少 1 副本）
# - 06-ingress.yaml      Ingress（HTTPS + cert-manager）
#
# 安全特性：
# - runAsNonRoot: true（容器以 65532 UID 运行）
# - readOnlyRootFilesystem: true（根文件系统只读）
# - capabilities.drop: ALL + add: []（最小权限）
# - seccompProfile: RuntimeDefault（限制系统调用）
# - PodDisruptionBudget: 1（自愿干扰时保持可用）
# - PodAntiAffinity: 避免同节点双副本
