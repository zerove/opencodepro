# SSE 事件

`GET /api/events` 提供 Server-Sent Events 流。相关端点：

| 端点 | 用途 |
|------|------|
| `GET /api/events` | 订阅事件流 |
| `POST /api/events/broadcast` | 由服务端/客户端广播自定义事件 |
| `GET /api/events/health` | SSE 连接数健康度 |

## 查询参数

| 参数 | 状态 | 说明 |
|------|------|------|
| `events` | **已生效** | 按事件类型过滤，逗号分隔，如 `?events=intervention,node_execution_started` |
| `sop_id` | **已生效** | 仅推送携带该 SOP ID 的事件 |
| `session_id` | **已生效** | 仅推送携带该会话 ID 的事件 |
| `heartbeat` | **已生效** | 心跳间隔秒数，默认 30，`0` 表示禁用 |
| `project_id` | 仅解析，**未生效** | 见下方「未生效参数」 |
| `last_event_id` | 仅解析，**未生效** | 见下方「未生效参数」 |

空值参数（如 `?sop_id=`）会被归一化为「不过滤」，而不是「只匹配空 ID」这种永不命中的条件。

多个维度是「与」关系：同时给出 `?sop_id=a&session_id=b` 时，事件必须**同时**满足两者才会推送。

## 维度的来源

事件通过 `SseEvent` 枚举传输。维度有两个来源：

- **枚举字段**：节点/SOP 执行类事件与 `session_message_added` 直接携带 `sop_id` / `session_id`；
- **payload 字段**：`Custom` 变体（由 `POST /api/events/broadcast` 及干预广播产生）没有枚举字段，维度从 `payload` 中同名键读取。

payload 中缺失该键、值为 `null`、或值不是字符串时，视为**无该维度**，不会被任何维度过滤匹配。

## 当前生产中实际广播的事件

这一节与枚举定义有出入，实现补齐前请以本节为准。

| 事件 | 来源 | 触发点 |
|------|------|--------|
| `intervention` | `Custom` | `POST /api/cognitive/map/:sop_id/intervene`，payload 含 `sop_id` |
| 调用方自定义 | `Custom` | `POST /api/events/broadcast` |

`SseEvent` 枚举还定义了认知迭代、节点执行、技能安装等 14 个变体，但**它们目前没有生产发送方**，仅在测试中构造。订阅这些事件类型不会收到数据。

同样地，`POST /api/execute/goal` 与 `/api/execute/composite` 执行 SOP 时，内核产生的 `ExecutionEvent`（节点开始/完成/失败、执行完成、认知步骤、微反思等）**不会**转发到 SSE：HTTP 层虽向内核传入了 `event_tx`，但只取了 sender 而丢弃 receiver，事件在发送端即被静默丢弃。这与 gRPC 的 `stream_events` 是两套独立通道。

## 未生效参数

`project_id` 与 `last_event_id` 会从 URL 解析但不参与过滤。保留它们是为了不破坏既有 API 契约 —— 移除字段会让携带这些参数的客户端得到 400/422 而非静默忽略。

- `project_id`：全局广播 channel 不区分项目，事件与工作区/JWT 的 `project_id` 之间没有绑定关系，实现过滤需要先定义归属规则。
- `last_event_id`：事件 ID 是每次推送生成的随机 UUID，重连时携带的 ID 没有可匹配的历史；补齐续传需要引入带序号的全局事件日志。

## 并发限制

SSE 连接数有上限（默认 64），超限返回 `429 Too Many Requests`。心跳与 keep-alive 均由服务端发送，断线由浏览器 `EventSource` 自动重连。

## 客户端示例

```javascript
const es = new EventSource('/api/events?sop_id=sop-abc');
es.addEventListener('intervention', (e) => {
  const data = JSON.parse(e.data);
  console.log('干预', data.action, 'applied =', data.applied);
});
```
