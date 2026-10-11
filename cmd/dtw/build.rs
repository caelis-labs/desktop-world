fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // CUA's static Swift bridges need the system Swift runtime at the final
        // Rust link, including test executables and independently installed CLI.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
}
