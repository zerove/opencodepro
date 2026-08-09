# 安全

## API 密钥认证

所有 API 端点需要通过以下方式认证：

1. **请求头**（Header）：`X-API-Key: <your-key>`
2. **Bearer**：`Authorization: Bearer <your-key>`

密钥与配置中的 `api.key` 字段比对。

## 外部目录权限

`external_dir_permissions` 配置控制引擎可访问的目录：

```toml
[tools]
external_dir_permissions = [
    { dir = "/project", write = true },
    { dir = "/tmp", write = true },
    { dir = "/home", write = false }
]
```

## 环境变量替换

配置值支持环境变量替换：

- `{env:VAR_NAME}` - 使用环境变量，缺失时报错
- `{env:VAR_NAME|default}` - 使用环境变量，带默认值回退
- `{env:VAR_NAME:}` - 使用环境变量，缺失时为空字符串

## RBAC（基于角色的访问控制）

角色：`admin`、`user`、`viewer`、`guest`

| 角色 | 权限 |
|------|------------|
| admin | 完全访问 |
| user | 执行、读取、写入 |
| viewer | 只读 |
| guest | 仅健康检查 |

## 会话隔离

会话按会话 ID 隔离。每个会话维护：

- 独立的消息历史
- 独立的记忆上下文
- 独立的 DAG 执行状态

## 沙箱执行

WASM 沙箱为工具执行提供进程隔离：

- 通过 wasmtime 配置限制内存
- 基于燃料的指令限制
- 不允许访问允许目录之外的文件系统
- 无网络访问（除非明确授予）