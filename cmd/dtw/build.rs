fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let target = std::env::var("TARGET").unwrap();
        let arch = if target.starts_with("aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        let swift_target = format!("{arch}-apple-macosx14.0");
        println!("cargo:rerun-if-changed=src/backends/macos/Scene.swift");
        println!("cargo:rerun-if-changed=src/backends/macos/Native.swift");
        let mut command = std::process::Command::new("xcrun");
        command.args([
            "swiftc",
            "-parse-as-library",
            "-emit-library",
            "-static",
            "-O",
            "-module-name",
            "DTWScene",
            "-target",
            &swift_target,
            "-module-cache-path",
            out.join("swift-cache").to_str().unwrap(),
            "-o",
            out.join("libdtw_scene.a").to_str().unwrap(),
            "src/backends/macos/Scene.swift",
            "src/backends/macos/Native.swift",
        ]);
        let status = command
            .status()
            .expect("launch Swift compiler for DTW scene extension");
        assert!(status.success(), "DTW scene extension compilation failed");
        println!("cargo:rustc-link-search=native={}", out.display());
        println!("cargo:rustc-link-lib=static=dtw_scene");
        for framework in [
            "AppKit",
            "ScreenCaptureKit",
            "ImageIO",
            "UniformTypeIdentifiers",
            "ApplicationServices",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
        // DTW's static Swift bridges need the system Swift runtime at the final
        // Rust link, including test executables and independently installed CLI.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
