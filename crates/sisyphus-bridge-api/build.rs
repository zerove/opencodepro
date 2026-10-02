// Generated sisyphus-bridge-api Rust code intentionally uses `*Update` suffixes
// (SessionUpdate, TodoUpdate, OperationUpdate, LogUpdate) to match protobuf
// field names in bridge.proto — this is the canonical prost naming convention.
// The aggregated `Event` enum is part of a protobuf `oneof` and cannot be renamed
// without breaking the wire protocol. Suppress the lint on the generated inner
// `bridge_event::Event` enum only.
//
// The attribute key must be the protobuf full path (sisyphus.bridge.BridgeEvent.event),
// not the Rust module path — prost-build matches on the proto descriptor name.
//
// 另有 `double_must_use`：tonic 的 `#[async_trait]` 给生成的 service trait 方法
// 加 `#[must_use]`，而其返回类型（boxed Future）本身已带 `#[must_use]`，clippy
// 1.99.0 起判为冗余。手写代码不受影响，但 `make ci-clippy` 的 `-D warnings`
// 全 workspace 检查会被生成代码卡住，而我们既不能改 tonic 也不能改 prost 的
// 输出。该 lint 已在 `src/lib.rs` 以 crate 级 `#![allow]` 放行——
// `server_attribute` / `client_attribute` 只作用于生成的 client/server 结构体，
// `type_attribute` 对 service 无效，故这里不重复配置。

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .type_attribute(
            "sisyphus.bridge.BridgeEvent.event",
            "#[allow(clippy::enum_variant_names)]",
        )
        // Suppress enum_variant_names on ClusterEvent oneof (generated enum from
        // peer_joined/peer_left/state_proposal/experience_sync — canonical
        // protobuf naming for the cluster wire protocol).
        .type_attribute(
            "sisyphus.cluster.ClusterEvent.event",
            "#[allow(clippy::enum_variant_names)]",
        )
        .compile_protos(
            &["proto/bridge.proto", "proto/cluster.proto"],
            &["proto"],
        )?;
    Ok(())
}
