fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_path = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
    ).join("../../crates/sisyphus-bridge/proto/bridge.proto");

    // Canonicalize to get an absolute path
    let proto_path = std::fs::canonicalize(&proto_path)?;

    tonic_build::compile_protos(&proto_path)?;

    Ok(())
}
