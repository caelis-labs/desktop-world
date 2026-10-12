fn main() {
    let output =
        std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded-bundle.tar.gz");
    println!("cargo:rerun-if-env-changed=DTW_EMBED_BUNDLE");
    if let Ok(source) = std::env::var("DTW_EMBED_BUNDLE") {
        println!("cargo:rerun-if-changed={source}");
        std::fs::copy(source, output).expect("copy embedded runtime bundle");
    } else {
        std::fs::write(output, []).expect("write empty development bundle");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // CUA's static Swift bridges need the system Swift runtime at the final
        // Rust link, including test executables and independently installed CLI.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
