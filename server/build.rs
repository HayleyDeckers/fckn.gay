use std::{fs, path::Path, process::Command};

fn main() {
    // Check if validation WASM files exist and are older than the validation crate
    let validation_dir = Path::new("../validation");
    let wasm_file = Path::new("static/fckn_gay_validation_bg.wasm");
    let js_file = Path::new("static/fckn_gay_validation.js");

    let needs_rebuild = !wasm_file.exists()
        || !js_file.exists()
        || validation_dir
            .join("src/lib.rs")
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            > wasm_file
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

    if needs_rebuild {
        println!("cargo:warning=Rebuilding WASM validation module...");

        // Change to validation directory and build
        let output = Command::new("wasm-pack")
            .args(["build", "--target", "web", "--out-dir", "pkg"])
            .current_dir(validation_dir)
            .output();

        match output {
            Ok(output) => {
                if !output.status.success() {
                    eprintln!(
                        "cargo:warning=Failed to build WASM module: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                } else {
                    println!("cargo:warning=WASM module built successfully");
                }
            }
            Err(e) => {
                eprintln!("cargo:warning=Failed to run wasm-pack: {}", e);
            }
        }

        // Copy WASM files to static directory using std::fs
        if let Ok(entries) = fs::read_dir(validation_dir.join("pkg")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension()
                    && (ext == "wasm" || ext == "js")
                    && let Some(filename) = path.file_name()
                {
                    let dest = Path::new("static").join(filename);
                    if let Err(e) = fs::copy(&path, &dest) {
                        eprintln!("cargo:warning=Failed to copy {:?}: {}", filename, e);
                    }
                }
            }
        }
    }

    // Tell cargo to rebuild if validation source changes
    println!("cargo:rerun-if-changed=../validation/src/lib.rs");
    println!("cargo:rerun-if-changed=../validation/Cargo.toml");
}
