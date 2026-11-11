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
| `project_id` | **已生效** | 仅推送携带该项目 ID 的事件 |
| `last_event_id` | 仅解析，**未生效** | 见下方「未生效参数」 |

空值参数（如 `?sop_id=`）会被归一化为「不过滤」，而不是「只匹配空 ID」这种永不命中的条件。

多个维度是「与」关系：同时给出 `?sop_id=a&session_id=b` 时，事件必须**同时**满足两者才会推送。

## 维度的来源

事件通过 `SseEvent` 枚举传输。维度有两个来源：

- **枚举字段**：节点/SOP 执行类事件与 `session_message_added` 直接携带 `sop_id` / `session_id`；
- **payload 字段**：`Custom` 变体（由 `POST /api/events/broadcast`、干预广播与执行事件转发器产生）没有枚举字段，维度从 `payload` 中同名键读取。

`POST /api/events/broadcast` 的请求体支持在**顶层**声明归属维度：

```json
{ "event_type": "custom", "sop_id": "sop-abc", "payload": { "message": "hi" } }
```

服务端会把顶层的 `sop_id` / `session_id` / `project_id` 写入事件 payload。若 payload 内已有同名键，**以顶层为准** —— 顶层字段是专门的归属声明，payload 只是业务载荷。顶层为空白字符串时视为「未声明」，payload 内已有维度保留。payload 不是对象（如 `null`、数组）时无法写入维度，事件原样送达。

payload 中缺失该键、值为 `null`、或值不是字符串时，视为**无该维度**，不会被任何维度过滤匹配。

## 当前生产中实际广播的事件

这一节与枚举定义有出入，实现补齐前请以本节为准。

| 事件 | 来源 | 触发点 |
|------|------|--------|
| `intervention` | `Custom` | `POST /api/cognitive/map/:sop_id/intervene`，payload 含 `sop_id` |
| 调用方自定义 | `Custom` | `POST /api/events/broadcast` |
| `node_started` | `Custom` | `POST /api/execute/goal` 执行期间，payload 为内核 `ExecutionEvent::NodeStarted` 原文加 `sop_id` |
| `node_completed` | `Custom` | 同上，`ExecutionEvent::NodeCompleted` |
| `node_failed` | `Custom` | 同上，`ExecutionEvent::NodeFailed` |
| `tool_call` | `Custom` | `POST /api/tools/execute`：`started` 与 `completed`/`failed` 成对发出，payload 含 `tool_id`、`phase`、`duration_ms`、`success`、`error`、`project_id`（来自 `x-project-id` 头，默认 `default`） |
| `execution_complete` | `Custom` | 同上，`ExecutionEvent::ExecutionComplete` |
| `cognitive_plan` | `Custom` | 同上，`ExecutionEvent::CognitivePlan` |
| `cognitive_step` | `Custom` | 同上，`ExecutionEvent::CognitiveStep` |
| `micro_reflection` | `Custom` | 同上，`ExecutionEvent::MicroReflection` |
| `cognitive_complete` | `Custom` | 同上，`ExecutionEvent::CognitiveComplete` |

`SseEvent` 枚举还定义了认知迭代、技能安装等变体，但**它们目前没有生产发送方**，仅在测试中构造。订阅这些事件类型不会收到数据。

内核 `ExecutionEvent` 共 10 个变体，全部以 `Custom` 事件转发（snake_case 命名），payload 保留原始字段（含 `fuel_consumed`、`duration_ms`）并注入 `sop_id`。事件名与 gRPC `stream_events` 是两套独立通道但共享同一批内核事件。

其中两个变体的生产方较特殊：

- `tool_call`：内核不产生，由 REST 工具执行端点直接广播（见上表）；
- `token_usage`：**当前仓库内没有生产发送方**。它由具备 usage 数据的一侧通过 `POST /api/events/broadcast`（`event_type = "token_usage"`）上报，或由未来的 provider 封装直接发出。订阅方必须按"可选事件"处理，不得假定一定出现。

CLI/TUI 的控制台事件视图已可渲染这两个变体（`TOOL` / `TOKENS` 行）；未识别的变体不会中断事件流。

## 执行端点与事件订阅

`POST /api/execute/goal` 的事件归属请求自身的 `sop_id`，因此订阅时用同一个值即可：

```bash
curl -N 'http://localhost:50051/api/events?sop_id=sop-abc' &
curl -X POST localhost:50051/api/execute/goal \
  -H 'content-type: application/json' \
  -d '{"sop_id":"sop-abc","goal":"...","event_tx":true}'
```

`POST /api/execute/composite` 内部各图的 `sop_id` 由内核生成、在 REST 层不可见，因此响应体额外返回 `sse_sop_id`（形如 `composite-<uuid>`）。客户端**必须用这个值**订阅，复合执行内部图的事件才能命中：

```bash
SID=$(curl -s -X POST localhost:50051/api/execute/composite \
  -H 'content-type: application/json' -d '{"...":"..."}' | jq -r .sse_sop_id)
curl -N "http://localhost:50051/api/events?sop_id=$SID"
```

由于 `sse_sop_id` 在执行开始时生成，需在请求进行中订阅；执行结束后订阅收不到已发出的事件（当前无事件重放，见「未生效参数」）。

## `event_tx` 参数

| 取值 | 行为 |
|------|------|
| 未传 / `null` | 转发 |
| `true` | 转发 |
| `false` | **完全不建立转发器** |

默认转发是刻意的：内核已经产生了这些事件，丢弃接收端只会让它们在发送端静默失败。

## 未生效参数

`last_event_id` 会从 URL 解析但不参与过滤。保留它是为了不破坏既有 API 契约 —— 移除字段会让携带该参数的客户端得到 400/422 而非静默忽略。

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
