use serde_json::Value;
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dtw"))
        .args(args)
        .output()
        .expect("run dtw")
}

#[test]
fn default_entry_exits_with_app_scoped_help() {
    for args in [&[][..], &["--help"], &["app", "--help"]] {
        let output = cli(args);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("dtw app catalog"));
        assert!(text.contains("dtw app serve --grant"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn version_is_available_without_worker_or_bundle() {
    let output = cli(&["--version"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        concat!("dtw ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn legacy_commands_are_rejected() {
    for args in [&["install", "plugin"][..], &["--stdio"], &["--serve"]] {
        let output = cli(args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "invalid_arguments");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn serve_requires_a_host_grant_before_native_startup() {
    let output = cli(&["app", "serve", "--grant", "relative.json"]);
    assert_eq!(output.status.code(), Some(2));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "invalid_grant");
}
