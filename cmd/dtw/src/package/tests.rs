use super::*;
use std::fs::OpenOptions;
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("dtw package 中文 {}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn help_and_invalid_arguments_do_not_touch_store() {
    let t = Temp::new();
    let blocker = t.0.join("not-a-directory");
    fs::write(&blocker, b"preserve me").unwrap();
    for store in [t.0.join("absent-store"), blocker.join("unwritable-store")] {
        for command in [vec![], vec!["--help"], vec!["-h"]] {
            let mut args: Vec<String> = command.into_iter().map(str::to_owned).collect();
            args.extend(["--store".into(), store.to_str().unwrap().into()]);
            let help = run(&args).unwrap();
            assert!(help["usage"].as_str().unwrap().contains("dtw package"));
            assert_eq!(help["manager_version"], env!("CARGO_PKG_VERSION"));
            assert_eq!(help["store_schema"], SCHEMA);
            assert!(!store.exists());
        }
        for command in [
            vec!["unknown"],
            vec!["import"],
            vec!["import", "--unknown"],
            vec!["status", "extra"],
            vec!["gc", "--unknown"],
            vec!["bind", "host", "not-a-package-id"],
            vec!["activate", "../invalid"],
            vec!["unbind", "../invalid"],
            vec!["--help", "extra"],
            vec!["--store", "duplicate"],
        ] {
            let mut args: Vec<String> = command.into_iter().map(str::to_owned).collect();
            args.extend(["--store".into(), store.to_str().unwrap().into()]);
            assert!(run(&args).is_err(), "{args:?}");
            assert!(!store.exists());
        }
    }
    assert_eq!(fs::read(&blocker).unwrap(), b"preserve me");
    assert_eq!(fs::read_dir(&t.0).unwrap().count(), 1);
    assert!(run(&["status".into(), "--store".into()]).is_err());
}

fn fixture(root: &Path, version: &str) -> PathBuf {
    fixture_layout(root, version, 1)
}

fn fixture_layout(root: &Path, version: &str, schema: u64) -> PathBuf {
    let skill_dir = match schema {
        1 => "skills/desktop-world",
        2 => "skills/dtw",
        _ => panic!("unsupported fixture schema"),
    };
    let p = root.join(format!("source-{version}"));
    fs::create_dir(&p).unwrap();
    fs::create_dir(p.join("bin")).unwrap();
    fs::write(p.join("bin/dtw"), version).unwrap();
    fs::create_dir_all(p.join(skill_dir)).unwrap();
    fs::create_dir_all(p.join("cordis")).unwrap();
    fs::write(p.join("cordis/index.mjs"), "export{}").unwrap();
    for name in ["SKILL.md", "API.md", "PATTERNS.md"] {
        fs::write(p.join(skill_dir).join(name), format!("{version} {name}")).unwrap();
    }
    for name in [
        "plugin.json",
        ".claude-plugin/plugin.json",
        ".antigravity-plugin/plugin.json",
        "package.json",
    ] {
        let f = p.join(name);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        write(&f, &json!({"version":version})).unwrap();
    }
    write(&p.join("BUILD.json"),&json!({"package_store_schema":schema,"version":version,"target":"aarch64-apple-darwin","binary":{"path":"bin/dtw","sha256":digest(&p.join("bin/dtw")).unwrap()},"skill_sha256":digest(&p.join(skill_dir).join("SKILL.md")).unwrap()})).unwrap();
    refresh_checksums(&p);
    p
}

fn refresh_checksums(p: &Path) {
    let rows = inventory(p)
        .unwrap()
        .iter()
        .filter(|(name, _)| name.as_str() != "SHA256SUMS")
        .map(|(name, path)| format!("{}  {name}\n", digest(path).unwrap()))
        .collect::<String>();
    fs::write(p.join("SHA256SUMS"), rows).unwrap();
}

#[test]
fn imports_linux_packages_for_both_native_architectures() {
    for target in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
        let t = Temp::new();
        let source = fixture_layout(&t.0, "0.5.3", 2);
        let mut build = json(&source.join("BUILD.json")).unwrap();
        build["target"] = target.into();
        fs::write(
            source.join("BUILD.json"),
            serde_json::to_vec(&build).unwrap(),
        )
        .unwrap();
        refresh_checksums(&source);
        let store = Store::open(&t.0.join("store")).unwrap();
        let imported = store.import(&source).unwrap();
        assert_eq!(imported["target"], target);
        let payload = Path::new(imported["package_path"].as_str().unwrap());
        let executable = Path::new(imported["executable"].as_str().unwrap());
        assert_eq!(executable, payload.join("bin").join("dtw"));
    }
}

#[test]
fn renamed_package_layout_preserves_legacy_pins_and_rollback() {
    let t = Temp::new();
    let old = fixture(&t.0, "0.4.2");
    let new = fixture_layout(&t.0, "0.4.3", 2);
    let old_id = digest(&old.join("SHA256SUMS")).unwrap();
    let store = Store::open(&t.0.join("store")).unwrap();
    assert_eq!(store.import(&old).unwrap()["package_id"], old_id);
    let new_id = store.import(&new).unwrap()["package_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(old_id, new_id);
    for host in ["dsh", "claude"] {
        store.bind(host, &old_id).unwrap();
        store.activate(host).unwrap();
    }
    store.bind("dsh", &new_id).unwrap();
    let pending = json(&store.host("dsh").unwrap()).unwrap();
    assert_eq!(pending["current"], old_id);
    assert_eq!(pending["pending"], new_id);
    store.activate("dsh").unwrap();
    assert_eq!(
        json(&store.host("dsh").unwrap()).unwrap()["rollback"],
        old_id
    );
    assert_eq!(
        json(&store.host("claude").unwrap()).unwrap()["current"],
        old_id
    );
    store.bind("dsh", &old_id).unwrap();
    store.activate("dsh").unwrap();
    assert_eq!(
        json(&store.host("dsh").unwrap()).unwrap()["current"],
        old_id
    );
    assert_eq!(digest(&old.join("SHA256SUMS")).unwrap(), old_id);
    assert!(old.join("skills/desktop-world/SKILL.md").is_file());
    assert!(!old.join("skills/dtw").exists());
    assert!(store.recipe(&old_id).is_ok());
    assert!(store.recipe(&new_id).is_ok());
}

#[test]
fn package_layout_requires_an_explicit_supported_schema_and_matching_skill() {
    let t = Temp::new();
    let p = fixture_layout(&t.0, "0.4.3", 2);
    let mut build = json(&p.join("BUILD.json")).unwrap();
    for schema in [
        Value::Null,
        json!(true),
        json!("2"),
        json!(2.0),
        json!(3),
        json!(1),
    ] {
        build["package_store_schema"] = schema;
        fs::write(p.join("BUILD.json"), serde_json::to_vec(&build).unwrap()).unwrap();
        refresh_checksums(&p);
        assert!(verify(&p).is_err());
    }
}
#[test]
fn import_rejects_both_canonical_skill_roots_for_each_layout() {
    let t = Temp::new();
    for schema in [1, 2] {
        let p = fixture_layout(&t.0, &format!("0.4.{schema}"), schema);
        let other = p.join(if schema == 1 {
            "skills/dtw"
        } else {
            "skills/desktop-world"
        });
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("SKILL.md"), "second discoverable Skill").unwrap();
        refresh_checksums(&p);
        let store = Store::open(&t.0.join(format!("store-{schema}"))).unwrap();
        assert!(store
            .import(&p)
            .unwrap_err()
            .message
            .contains("Skill layout mismatch"));
        assert_eq!(
            fs::read_dir(store.root.join("packages")).unwrap().count(),
            0
        );
    }
}

#[test]
fn complete_import_deduplicates_and_encodes_unicode_file_urls() {
    let t = Temp::new();
    let source = fixture(&t.0, "0.3.1");
    let store = Store::open(&t.0.join("managed store")).unwrap();
    let a = store.import(&source).unwrap();
    let b = store.import(&source).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        fs::read_dir(store.root.join("packages")).unwrap().count(),
        1
    );
    let url = url::Url::parse(a["cordis_url"].as_str().unwrap()).unwrap();
    assert!(url.as_str().contains("%20") && url.as_str().contains("%E4"));
    assert_eq!(
        fs::canonicalize(url.to_file_path().unwrap()).unwrap(),
        fs::canonicalize(Path::new(a["package_path"].as_str().unwrap()).join("cordis/index.mjs"))
            .unwrap()
    );
}
#[test]
fn staggered_hosts_pending_rollback_and_live_leases_protect_exact_packages() {
    let t = Temp::new();
    let s1 = fixture(&t.0, "0.3.0");
    let s2 = fixture(&t.0, "0.3.1");
    let s3 = fixture(&t.0, "0.3.2");
    let store = Store::open(&t.0.join("store")).unwrap();
    let a = store.import(&s1).unwrap()["package_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let b = store.import(&s2).unwrap()["package_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let c = store.import(&s3).unwrap()["package_id"]
        .as_str()
        .unwrap()
        .to_owned();
    store.bind("agy", &a).unwrap();
    store.activate("agy").unwrap();
    store.bind("dsh-desktop", &b).unwrap();
    store.activate("dsh-desktop").unwrap();
    store.bind("agy", &c).unwrap();
    let gc = store.gc(false, 0).unwrap();
    assert!(gc["packages"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["retained_by"] == "host_pin"));
    store.activate("agy").unwrap();
    assert_eq!(json(&store.host("agy").unwrap()).unwrap()["rollback"], a);
    fs::remove_file(store.host("agy").unwrap()).unwrap();
    let lease_path = store.root.join("leases").join(format!("{a}.lock"));
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lease_path)
        .unwrap();
    lease.try_lock_shared().unwrap();
    let gc = store.gc(true, 0).unwrap();
    assert!(gc["packages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["package_id"] == a && r["retained_by"] == "running"));
    assert!(store.package(&a).is_ok());
    assert!(store.package(&b).is_ok());
    assert!(store.package(&c).is_err());
    drop(lease);
    store.gc(true, 0).unwrap();
    assert!(store.package(&a).is_err());
    assert!(store.package(&b).is_ok());
}
#[test]
fn corrupt_or_unlisted_files_and_unsafe_members_are_rejected() {
    let t = Temp::new();
    let p = fixture(&t.0, "0.3.1");
    fs::write(p.join("skills/desktop-world/API.md"), "wrong").unwrap();
    assert!(verify(&p).unwrap_err().message.contains("checksum"));
    for path in [
        "../other",
        "/absolute",
        "C:/evil",
        "a\\b",
        "a//b",
        "x/./y",
        "trailing.",
        "a/b ",
        "CON",
        "aux.txt",
        "x/COM1.exe",
    ] {
        assert!(relative(path).is_err(), "{path}");
    }
    let other = t.0.join("unmanaged");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("keep"), "data").unwrap();
    assert!(Store::open(&other).is_err());
    assert!(other.join("keep").exists());
}
#[cfg(unix)]
#[test]
fn import_and_gc_refuse_symbolic_package_roots() {
    use std::os::unix::fs::symlink;
    let t = Temp::new();
    let p = fixture(&t.0, "0.3.1");
    let outside = t.0.join("outside");
    fs::write(&outside, "preserve").unwrap();
    fs::remove_file(p.join("skills/desktop-world/API.md")).unwrap();
    symlink(&outside, p.join("skills/desktop-world/API.md")).unwrap();
    assert!(verify(&p).is_err());
    assert_eq!(fs::read_to_string(outside).unwrap(), "preserve");
}
#[test]
fn binding_failure_keeps_original_reference_and_new_package_pending() {
    let t = Temp::new();
    let source = fixture(&t.0, "0.3.1");
    let store = Store::open(&t.0.join("store")).unwrap();
    let id = store.import(&source).unwrap()["package_id"]
        .as_str()
        .unwrap()
        .to_owned();
    store.bind("dsh", &id).unwrap();
    assert!(json(&store.host("dsh").unwrap()).unwrap()["current"].is_null());
    assert_eq!(store.gc(true, 0).unwrap()["packages"][0]["action"], "keep");
    assert!(store.bind("../escape", &id).is_err());
}

#[test]
fn malformed_host_references_stop_collection_without_removing_packages() {
    let t = Temp::new();
    let source = fixture(&t.0, "0.3.1");
    let store = Store::open(&t.0.join("store")).unwrap();
    let id = store.import(&source).unwrap()["package_id"]
        .as_str()
        .unwrap()
        .to_owned();
    write(
        &store.host("dsh").unwrap(),
        &json!({"host":"dsh","current":42}),
    )
    .unwrap();
    assert!(store.gc(true, 0).is_err());
    assert!(store.package(&id).is_ok());
    for host in ["DSH", "con", "nul.profile", "lpt9"] {
        assert!(store.bind(host, &id).is_err());
    }
}

#[cfg(unix)]
#[test]
fn dangling_locks_and_payload_links_never_create_or_remove_external_data() {
    use std::os::unix::fs::symlink;
    let t = Temp::new();
    let root = t.0.join("store");
    drop(Store::open(&root).unwrap());
    fs::remove_file(root.join("store.lock")).unwrap();
    let outside = t.0.join("never-created");
    symlink(&outside, root.join("store.lock")).unwrap();
    assert!(Store::open(&root).is_err());
    assert!(!outside.exists());
    let stage = t.0.join("stage");
    fs::create_dir(&stage).unwrap();
    write(
        &stage.join("STAGE.json"),
        &json!({"kind":"dtw-owned-stage"}),
    )
    .unwrap();
    symlink(&outside, stage.join("payload")).unwrap();
    assert!(remove_owned_tree(&stage).is_err());
    assert!(stage.join("STAGE.json").exists());
    assert!(!outside.exists());
}

fn setup_command(
    root: &Path,
    command: &[&str],
    manager: Option<&Path>,
    register: impl FnOnce(&str, &Value, Option<&Value>, &str) -> std::result::Result<Value, String>,
) -> Result<Value> {
    let mut args: Vec<_> = command.iter().map(|part| part.to_string()).collect();
    args.extend(["--store".into(), text(root).unwrap().into()]);
    setup::run_with(
        &args,
        None,
        || Ok(manager.expect("unexpected manager selection").to_path_buf()),
        register,
    )
}

fn no_registration(
    _: &str,
    _: &Value,
    _: Option<&Value>,
    _: &str,
) -> std::result::Result<Value, String> {
    panic!("unexpected host registration")
}

#[test]
fn setup_checks_download_and_rejects_wrong_source_before_import_or_binding() {
    let t = Temp::new();
    let root = t.0.join("store");
    let source = fixture(&t.0, "0.5.4");
    let binary = source.join("bin/dtw");
    let id = digest(&source.join("SHA256SUMS")).unwrap();
    let wrong_id = "0".repeat(64);
    assert_eq!(
        setup_command(&root, &["resume", "pi", "pi"], None, no_registration).unwrap()["state"],
        "no_pending"
    );
    let missing =
        setup_command(&root, &["prepare", "pi", "pi", &id], None, no_registration).unwrap();
    assert_eq!(missing["state"], "needs_download");
    assert_eq!(missing["package_id"], id);
    assert!(missing.get("install").is_none());
    let wrong = setup_command(
        &root,
        &["prepare", "pi", "pi", &wrong_id, text(&source).unwrap()],
        None,
        no_registration,
    )
    .unwrap_err();
    assert!(wrong.message.contains("does not match expected package"));

    let another = fixture(&t.0, "0.5.3").join("bin/dtw");
    let args = ["prepare", "pi", "pi", &id, text(&source).unwrap()];
    let wrong_manager = setup_command(&root, &args, Some(&another), no_registration).unwrap_err();
    assert!(wrong_manager
        .message
        .contains("verified source package executable"));
    fs::write(source.join("cordis/index.mjs"), "tampered").unwrap();
    let damaged = setup_command(&root, &args, Some(&binary), no_registration).unwrap_err();
    assert!(damaged.message.contains("checksum mismatch"));
    assert_eq!(fs::read_dir(root.join("packages")).unwrap().count(), 0);
    assert!(Store::open(&root).unwrap().bindings().unwrap().is_empty());
}

#[test]
fn setup_serializes_registration_and_confirms_only_the_expected_verified_pin() {
    let t = Temp::new();
    let root = t.0.join("store");
    let a = fixture(&t.0, "0.5.4");
    let b = fixture(&t.0, "0.5.5");
    let a_id = digest(&a.join("SHA256SUMS")).unwrap();
    let b_id = digest(&b.join("SHA256SUMS")).unwrap();
    let prepare_a = setup_command(
        &root,
        &[
            "prepare",
            "dsh-desktop",
            "dsh-desktop",
            &a_id,
            text(&a).unwrap(),
        ],
        Some(&a.join("bin/dtw")),
        |host, install, previous, mode| {
            assert_eq!(host, "dsh-desktop");
            assert_eq!(mode, "new");
            assert_eq!(install["package_id"], a_id);
            assert!(previous.is_none());
            let store = Store::open(&root).unwrap();
            assert_eq!(
                store.read_host("dsh-desktop").unwrap().unwrap()["pending"],
                a_id
            );
            assert!(store
                .activate("dsh-desktop")
                .unwrap_err()
                .message
                .contains("busy"));
            drop(store);
            assert!(setup_command(
                &root,
                &["confirm", "dsh-desktop", "dsh-desktop", &a_id],
                None,
                no_registration,
            )
            .unwrap_err()
            .message
            .contains("busy"));
            Ok(json!({"state":"restart_required","next":"Restart the host","backup":"original"}))
        },
    )
    .unwrap();
    assert_eq!(prepare_a["state"], "restart_required");
    assert_eq!(prepare_a["registration"]["backup"], "original");
    let confirm_a = ["confirm", "dsh-desktop", "dsh-desktop", &a_id];
    let first = setup_command(&root, &confirm_a, None, no_registration).unwrap();
    assert_eq!(first["state"], "completed");
    assert_eq!(first["binding"]["current"], a_id);
    let path = root.join("hosts/dsh-desktop.json");
    let confirmed_bytes = fs::read(&path).unwrap();
    let prepare_current = setup_command(
        &root,
        &["prepare", "dsh-desktop", "dsh-desktop", &a_id],
        None,
        |_, install, previous, mode| {
            assert_eq!(mode, "current");
            assert_eq!(install["package_id"], a_id);
            assert!(previous.is_none());
            Ok(json!({"state":"host_action_required","reason":"Missing registration"}))
        },
    )
    .unwrap();
    assert_eq!(prepare_current["state"], "host_action_required");
    assert_eq!(
        setup_command(&root, &confirm_a, None, no_registration).unwrap()["state"],
        "completed"
    );
    assert_eq!(fs::read(&path).unwrap(), confirmed_bytes);

    let interrupted = setup_command(
        &root,
        &[
            "prepare",
            "dsh-desktop",
            "dsh-desktop",
            &b_id,
            text(&b).unwrap(),
        ],
        Some(&b.join("bin/dtw")),
        |_, _, previous, mode| {
            assert_eq!(mode, "new");
            let previous = previous.unwrap();
            assert_eq!(previous["package_id"], a_id);
            assert!(previous["package_path"].is_string());
            assert!(previous["cordis_url"].is_string());
            Err("Fixture interrupted registration".into())
        },
    );
    assert_eq!(
        interrupted.unwrap_err().message,
        "Fixture interrupted registration"
    );
    assert!(setup_command(&root, &confirm_a, None, no_registration)
        .unwrap_err()
        .message
        .contains("does not match expected package"));
    let resume = setup_command(
        &root,
        &["resume", "dsh-desktop", "dsh-desktop"],
        Some(&b.join("bin/dtw")),
        |_, _, previous, mode| {
            assert_eq!(mode, "resume");
            assert_eq!(previous.unwrap()["package_id"], a_id);
            Ok(json!({"state":"verify"}))
        },
    )
    .unwrap();
    assert_eq!(resume["state"], "verify");
    assert_eq!(resume["package_id"], b_id);
    let install = Path::new(resume["install"]["package_path"].as_str().unwrap());
    let member = install.join("cordis/index.mjs");
    let original = fs::read(&member).unwrap();
    fs::write(&member, "damaged").unwrap();
    let confirm_b = ["confirm", "dsh-desktop", "dsh-desktop", &b_id];
    assert!(setup_command(&root, &confirm_b, None, no_registration).is_err());
    fs::write(&member, original).unwrap();
    let confirmed = setup_command(&root, &confirm_b, None, no_registration).unwrap();
    assert_eq!(confirmed["binding"]["current"], b_id);
    assert_eq!(confirmed["binding"]["rollback"], a_id);
    assert_eq!(confirmed["binding"]["setup_host"], "dsh-desktop");
    assert!(confirmed["binding"]["pending"].is_null());
}

#[test]
fn setup_resumes_legacy_agy_binding_and_shares_the_completed_pin() {
    let t = Temp::new();
    let root = t.0.join("store");
    let source = fixture(&t.0, "0.5.4");
    let id = digest(&source.join("SHA256SUMS")).unwrap();
    let store = Store::open(&root).unwrap();
    store.import(&source).unwrap();
    store.bind("agy", &id).unwrap();
    drop(store);
    for _ in 0..2 {
        let result = setup_command(
            &root,
            &["resume", "agy-cli", "agy"],
            None,
            |host, _, _, mode| {
                assert_eq!(host, "agy-cli");
                assert_eq!(mode, "resume");
                Ok(json!({"state":"verify"}))
            },
        )
        .unwrap();
        assert_eq!(result["package_id"], id);
    }
    assert!(setup_command(&root, &["resume", "agy-app", "agy"], None, no_registration).is_err());
    setup_command(
        &root,
        &["confirm", "agy-cli", "agy", &id],
        None,
        no_registration,
    )
    .unwrap();
    assert_eq!(
        setup_command(&root, &["resume", "agy-app", "agy"], None, no_registration).unwrap()
            ["state"],
        "no_pending"
    );
    let updated = fixture(&t.0, "0.5.5");
    let updated_id = digest(&updated.join("SHA256SUMS")).unwrap();
    setup_command(
        &root,
        &[
            "prepare",
            "agy-app",
            "agy",
            &updated_id,
            text(&updated).unwrap(),
        ],
        Some(&updated.join("bin/dtw")),
        |host, _, previous, mode| {
            assert_eq!(host, "agy-app");
            assert_eq!(mode, "new");
            assert_eq!(previous.unwrap()["package_id"], id);
            Ok(json!({"state":"verify"}))
        },
    )
    .unwrap();
    assert!(setup_command(&root, &["resume", "agy-cli", "agy"], None, no_registration).is_err());
}

#[test]
fn setup_reserves_custom_profiles_for_manual_registration() {
    let t = Temp::new();
    let root = t.0.join("store");
    let source = fixture(&t.0, "0.5.4");
    let id = digest(&source.join("SHA256SUMS")).unwrap();
    let manual = setup_command(
        &root,
        &["prepare", "pi", "pi-work", &id, text(&source).unwrap()],
        Some(&source.join("bin/dtw")),
        no_registration,
    )
    .unwrap();
    assert_eq!(manual["state"], "host_action_required");
    assert_eq!(manual["package_id"], id);
    assert!(manual["next"].is_string());
    assert!(setup_command(
        &root,
        &["resume", "opencode", "pi-work"],
        None,
        no_registration
    )
    .is_err());
    let done = setup_command(
        &root,
        &["confirm", "pi", "pi-work", &id],
        None,
        no_registration,
    )
    .unwrap();
    assert_eq!(done["binding"]["setup_host"], "pi");
}

#[test]
fn delegated_resume_rejects_a_pending_selection_that_changed_during_handoff() {
    let t = Temp::new();
    let root = t.0.join("store");
    let source = fixture(&t.0, "0.5.4");
    let id = digest(&source.join("SHA256SUMS")).unwrap();
    let store = Store::open(&root).unwrap();
    store.import(&source).unwrap();
    store.bind("pi", &id).unwrap();
    drop(store);
    let args = ["resume", "pi", "pi", "--store", text(&root).unwrap()].map(String::from);
    let result = setup::run_with(
        &args,
        Some(&"0".repeat(64)),
        || panic!("unexpected manager selection"),
        no_registration,
    );
    assert!(result
        .unwrap_err()
        .message
        .contains("changed before the selected manager"));
    assert_eq!(
        Store::open(&root)
            .unwrap()
            .read_host("pi")
            .unwrap()
            .unwrap()["pending"],
        id
    );
}

#[test]
fn store_io_error_reports_raw_code_and_execution_context() {
    let store = std::env::temp_dir().join("denied");
    let error = io_error(std::io::Error::from_raw_os_error(5)).context(
        "open_store",
        "import",
        &store,
        Some("verified"),
    );
    let details = error.details.unwrap();
    assert_eq!(details["phase"], "open_store");
    assert_eq!(details["command"], "import");
    assert_eq!(details["store"], json!(store));
    assert_eq!(details["source"], "verified");
    assert_eq!(details["os_error"], 5);
    assert!(details["executable"].is_string());
}
