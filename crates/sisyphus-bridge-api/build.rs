// Generated sisyphus-bridge-api Rust code intentionally uses `*Update` suffixes
// (SessionUpdate, TodoUpdate, OperationUpdate, LogUpdate) to match protobuf
// field names in bridge.proto — this is the canonical prost naming convention.
// The aggregated `Event` enum is part of a protobuf `oneof` and cannot be renamed
// without breaking the wire protocol. Suppress the lint on the generated inner
// `bridge_event::Event` enum only.
//
// The attribute key must be the protobuf full path (sisyphus.bridge.BridgeEvent.event),
// not the Rust module path — prost-build matches on the proto descriptor name.
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
