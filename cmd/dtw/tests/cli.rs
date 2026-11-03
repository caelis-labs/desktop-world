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
        assert!(text.contains("dtw mcp"));
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
    for args in [
        &["install", "plugin"][..],
        &["--stdio"],
        &["--serve"],
        &["api"],
    ] {
        let output = cli(args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "invalid_arguments");
    }
}

#[test]
fn package_help_and_argument_errors_are_json_without_store_access() {
    let root = std::env::temp_dir().join(format!("dtw-cli-package-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let blocker = root.join("not-a-directory");
    std::fs::write(&blocker, b"preserve me").unwrap();
    for store in [root.join("absent-store"), blocker.join("unwritable-store")] {
        let path = store.to_str().unwrap();
        for help in ["--help", "-h"] {
            let output = cli(&["package", help, "--store", path]);
            assert!(output.status.success(), "{:?}", output);
            assert!(output.stderr.is_empty());
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(value["usage"].as_str().unwrap().contains("dtw package"));
            assert_eq!(value["manager_version"], env!("CARGO_PKG_VERSION"));
            assert_eq!(value["store_schema"], 1);
            assert!(!store.exists());
        }
        for command in [
            &["unknown"][..],
            &["import"][..],
            &["status", "extra"][..],
            &["bind", "host", "invalid"][..],
            &["activate", "../invalid"][..],
        ] {
            let mut args = vec!["package"];
            args.extend_from_slice(command);
            args.extend_from_slice(&["--store", path]);
            let output = cli(&args);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            let error: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(error["version"], 1);
            assert_eq!(error["error"]["code"], "package_error");
            assert!(!store.exists());
        }
    }
    assert_eq!(std::fs::read(&blocker).unwrap(), b"preserve me");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();

    // Even default-store environment resolution must wait until dispatch.
    for command in ["--help", "unknown"] {
        let output = Command::new(env!("CARGO_BIN_EXE_dtw"))
            .args(["package", command])
            .env_remove("LOCALAPPDATA")
            .env_remove("HOME")
            .env_remove("XDG_DATA_HOME")
            .output()
            .unwrap();
        if command == "--help" {
            assert!(output.status.success());
            let help: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(help["usage"].is_string());
        } else {
            assert_eq!(output.status.code(), Some(2));
            let error: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert!(error["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with("Usage:"));
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn removed_grant_entry_is_rejected() {
    let output = cli(&["app", "serve", "--grant", "relative.json"]);
    assert_eq!(output.status.code(), Some(2));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "invalid_arguments");
}
