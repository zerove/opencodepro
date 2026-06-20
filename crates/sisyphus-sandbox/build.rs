/// Build script: compiles all .wat files in the tools/ directory to .wasm.
/// Uses the `wat` crate cached locally.
use std::path::Path;

fn main() {
    let tools_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools")
        .canonicalize()
        .expect("tools/ directory not found");

    if !tools_dir.exists() {
        println!("cargo:warning=tools directory not found at {:?}", tools_dir);
        return;
    }

    for entry in std::fs::read_dir(&tools_dir).expect("Failed to read tools dir") {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();

        if path.extension().is_some_and(|e| e == "wat") {
            let wasm_path = path.with_extension("wasm");

            // Skip if .wasm is newer than .wat
            if wasm_path.exists() {
                let wat_modified = path.metadata().unwrap().modified().unwrap();
                let wasm_modified = wasm_path.metadata().unwrap().modified().unwrap();
                if wasm_modified > wat_modified {
                    continue;
                }
            }

            let wat_content = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("Failed to read {:?}", path));
            let wasm_bytes = wat::parse_str(&wat_content)
                .unwrap_or_else(|_| panic!("Failed to parse {:?}", path));
            std::fs::write(&wasm_path, wasm_bytes)
                .unwrap_or_else(|_| panic!("Failed to write {:?}", wasm_path));

            println!("cargo:rerun-if-changed={}", path.display());
            println!("cargo:warning=Compiled {} -> {}", 
                path.file_name().unwrap().to_str().unwrap(),
                wasm_path.file_name().unwrap().to_str().unwrap());
        }
    }
}
