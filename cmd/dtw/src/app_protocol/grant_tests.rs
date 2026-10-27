use super::*;

fn wall() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}
fn configuration() -> Value {
    json!({"version":1,"grant_id":"grant-a",
        "app":{"identity":"verified-identity","pid":42,"generation":"18446744073709551615"},
        "capabilities":["observe","press","capture","click"],
        "expires_at_unix_ms":1_800_000_060_000_u64})
}
fn decode(value: Value) -> Result<HostGrant, WireError> {
    parse(value, wall(), Instant::now())
}
fn code(value: Value) -> String {
    match decode(value) {
        Err(error) => error.code,
        Ok(_) => panic!("unexpected grant admission"),
    }
}

#[test]
fn exact_instance_and_capabilities_survive_configuration_and_description() {
    let now = Instant::now();
    let grant = parse(configuration(), wall(), now).unwrap();
    assert_eq!(grant.grant_id, "grant-a");
    assert_eq!(
        grant.app,
        AppInstance {
            identity: AppIdentity("verified-identity".into()),
            pid: 42,
            generation: u64::MAX
        }
    );
    assert_eq!(grant.expires_at_unix_ms, 1_800_000_060_000);
    assert_eq!(grant.expires_at, now + Duration::from_secs(60));
    assert_eq!(app_json(&grant.app), configuration()["app"]);
    assert_eq!(
        capabilities_json(&grant.capabilities),
        configuration()["capabilities"]
    );
}

#[test]
fn expiry_is_absolute_and_reloading_does_not_renew_it() {
    let now = Instant::now();
    let first = parse(configuration(), wall(), now).unwrap();
    let elapsed = Duration::from_secs(40);
    let reloaded = parse(configuration(), wall() + elapsed, now + elapsed).unwrap();
    assert_eq!(reloaded.expires_at, first.expires_at);
    for elapsed in [Duration::from_secs(60), Duration::from_secs(61)] {
        let error = match parse(configuration(), wall() + elapsed, now + elapsed) {
            Err(error) => error,
            Ok(_) => panic!("expired grant was renewed"),
        };
        assert_eq!(error.code, "grant_expired");
    }
}

#[test]
fn expiry_keeps_fractional_wall_time_and_accepts_at_most_24_hours() {
    let now = Instant::now();
    let grant = parse(configuration(), wall() + Duration::from_micros(500), now).unwrap();
    assert_eq!(
        grant.expires_at,
        now + Duration::from_millis(59_999) + Duration::from_micros(500)
    );
    let mut request = configuration();
    request["expires_at_unix_ms"] = json!(1_800_000_000_000_u64 + MAX_VALIDITY.as_millis() as u64);
    assert_eq!(
        parse(request.clone(), wall(), now).unwrap().expires_at,
        now + MAX_VALIDITY
    );
    request["expires_at_unix_ms"] = json!(1_800_000_000_001_u64 + MAX_VALIDITY.as_millis() as u64);
    assert_eq!(code(request), "invalid_grant");
    for expiry in [json!(0), json!(1_800_000_000_000_u64)] {
        let mut request = configuration();
        request["expires_at_unix_ms"] = expiry;
        assert_eq!(code(request), "grant_expired");
    }
}

#[test]
fn schema_requires_every_field_and_rejects_unknown_authority() {
    for value in [Value::Null, json!([]), json!("grant"), json!({})] {
        assert_eq!(code(value), "invalid_grant");
    }
    let base = configuration();
    for key in base.as_object().unwrap().keys() {
        let mut value = base.clone();
        value.as_object_mut().unwrap().remove(key);
        assert_eq!(code(value), "invalid_grant");
    }
    for key in base["app"].as_object().unwrap().keys() {
        let mut value = base.clone();
        value["app"].as_object_mut().unwrap().remove(key);
        assert_eq!(code(value), "invalid_grant");
    }
    for key in ["approved", "expires_after", "window", "provider", "allow"] {
        let mut value = base.clone();
        value[key] = json!(true);
        assert_eq!(code(value), "invalid_grant");
        let mut value = base.clone();
        value["app"][key] = json!(true);
        assert_eq!(code(value), "invalid_grant");
    }
    for version in [
        json!(null),
        json!(true),
        json!("1"),
        json!(0),
        json!(2),
        json!(1.5),
    ] {
        let mut value = base.clone();
        value["version"] = version;
        assert_eq!(code(value), "invalid_grant");
    }
}

#[test]
fn capabilities_are_nonempty_exact_and_never_silently_deduplicated() {
    for capabilities in [
        json!([]),
        json!(null),
        json!("observe"),
        json!(["observe", "observe"]),
        json!(["observe", "press", "click", "capture", "press"]),
        json!(["Observe"]),
        json!(["launch"]),
        json!(["observe", null]),
        json!([1]),
    ] {
        let mut value = configuration();
        value["capabilities"] = capabilities;
        assert_eq!(code(value), "invalid_grant");
    }
    for capability in ["observe", "press", "capture", "click"] {
        let mut value = configuration();
        value["capabilities"] = json!([capability]);
        assert_eq!(
            capabilities_json(&decode(value).unwrap().capabilities),
            json!([capability])
        );
    }
}

#[test]
fn identity_and_grant_names_are_bounded_without_normalizing_identity() {
    for (path, limit) in [("/grant_id", 128), ("/app/identity", 4096)] {
        for length in [1, limit] {
            let text = "界".repeat(length);
            let mut value = configuration();
            *value.pointer_mut(path).unwrap() = json!(text);
            assert!(decode(value).is_ok());
        }
        for replacement in [
            json!(""),
            json!("界".repeat(limit + 1)),
            json!(null),
            json!(1),
            json!(true),
        ] {
            let mut value = configuration();
            *value.pointer_mut(path).unwrap() = replacement;
            assert_eq!(code(value), "invalid_grant");
        }
    }
    let mut value = configuration();
    value["app"]["identity"] = json!(" identity with spaces ");
    assert_eq!(
        decode(value).unwrap().app.identity.0,
        " identity with spaces "
    );
}

#[test]
fn pid_and_decimal_generation_are_positive_and_lossless() {
    for pid in [
        json!(0),
        json!(-1),
        json!(u64::MAX),
        json!(1.0),
        json!("42"),
        json!(null),
        json!(true),
    ] {
        let mut value = configuration();
        value["app"]["pid"] = pid;
        assert_eq!(code(value), "invalid_grant");
    }
    for generation in [
        json!("0"),
        json!(""),
        json!("-1"),
        json!("+1"),
        json!("1.0"),
        json!("1e3"),
        json!(" 1"),
        json!("１"),
        json!("18446744073709551616"),
        json!(1),
        json!(null),
    ] {
        let mut value = configuration();
        value["app"]["generation"] = generation;
        assert_eq!(code(value), "invalid_grant");
    }
    for generation in ["1", "9007199254740993", "18446744073709551615"] {
        let mut value = configuration();
        value["app"]["generation"] = json!(generation);
        assert_eq!(
            app_json(&decode(value).unwrap().app)["generation"],
            generation
        );
    }
}

#[test]
fn expiry_requires_an_unsigned_json_integer() {
    for expiry in [
        json!("1800000060000"),
        json!(null),
        json!(true),
        json!(-1),
        json!(1.5),
        json!(u64::MAX),
    ] {
        let mut value = configuration();
        value["expires_at_unix_ms"] = expiry;
        assert_eq!(code(value), "invalid_grant");
    }
}

struct ReadBackend;
impl crate::computer::Backend for ReadBackend {
    type Binding = ();
    fn stamp(&self) -> crate::computer::ProviderStamp {
        crate::computer::ProviderStamp {
            id: "test".into(),
            generation: 1,
        }
    }
    fn supports(&self, _: Capability) -> bool {
        true
    }
    fn windows(
        &self,
        _: &AppInstance,
        _: &crate::computer::CallContext<'_>,
    ) -> Result<crate::computer::WindowInventory, crate::computer::Error> {
        Ok(crate::computer::test_inventory(vec![]))
    }
    fn observe(
        &self,
        _: &crate::computer::WindowTarget,
        _: &crate::computer::CallContext<'_>,
    ) -> Result<crate::computer::BackendObservation<()>, crate::computer::Error> {
        panic!("not used")
    }
    fn press(
        &self,
        _: &crate::computer::WindowTarget,
        _: &(),
        _: crate::computer::ElementId,
        _: &crate::computer::CallContext<'_>,
    ) -> crate::computer::ActionOutcome {
        panic!("not used")
    }
}
struct FixedClock(Instant);
impl crate::runtime::Clock for FixedClock {
    fn now(&self) -> Instant {
        self.0
    }
}

#[test]
fn core_grant_copies_the_original_capabilities_and_deadline() {
    let now = Instant::now();
    let host = parse(configuration(), wall(), now).unwrap();
    let before = crate::runtime::Runtime::with_clock(
        ReadBackend,
        host.core_grant(),
        Duration::from_secs(5),
        FixedClock(now),
    );
    assert_eq!(
        before.windows(),
        Ok(crate::computer::test_inventory(vec![]))
    );
    let expired = crate::runtime::Runtime::with_clock(
        ReadBackend,
        host.core_grant(),
        Duration::from_secs(5),
        FixedClock(host.expires_at),
    );
    assert_eq!(expired.windows(), Err(crate::computer::Error::GrantExpired));
    let mut value = configuration();
    value["capabilities"] = json!(["click"]);
    let limited = parse(value, wall(), now).unwrap();
    let runtime = crate::runtime::Runtime::with_clock(
        ReadBackend,
        limited.core_grant(),
        Duration::from_secs(5),
        FixedClock(now),
    );
    assert_eq!(
        runtime.windows(),
        Err(crate::computer::Error::CapabilityDenied)
    );
}

#[cfg(target_os = "macos")]
mod files {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Fixture {
        root: std::path::PathBuf,
        path: std::path::PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            let root = std::env::temp_dir().join(format!(
                "dtw-host-grant-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let path = root.join("grant.json");
            let fixture = Self { root, path };
            let mut value = configuration();
            value["expires_at_unix_ms"] =
                json!((SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
                    + Duration::from_secs(60))
                .as_millis() as u64);
            fixture.write(&value.to_string());
            fixture
        }
        fn write(&self, bytes: &str) {
            fs::write(&self.path, bytes).unwrap();
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn failure(path: &Path) -> WireError {
        match load(path) {
            Err(error) => error,
            Ok(_) => panic!("unsafe file was admitted"),
        }
    }

    #[test]
    fn loading_freezes_the_selected_bytes_and_deletion_does_not_change_the_grant() {
        let file = Fixture::new();
        let loaded = load(&file.path).unwrap();
        let expiry = loaded.expires_at;
        file.write("{}");
        assert_eq!(failure(&file.path).code, "invalid_grant");
        fs::remove_file(&file.path).unwrap();
        assert_eq!(loaded.app.generation, u64::MAX);
        assert_eq!(loaded.expires_at, expiry);
        assert_eq!(loaded.grant_id, "grant-a");
        assert_eq!(
            loaded.capabilities,
            vec![
                Capability::Observe,
                Capability::Press,
                Capability::Capture,
                Capability::Click
            ]
        );
    }

    #[test]
    fn permissions_must_be_exactly_private_0600() {
        let file = Fixture::new();
        // This host strips set-ID bits when chmod is applied. Exercise actual
        // stored modes, not a requested set-ID mode that the OS changed to 0600.
        for mode in [0o644, 0o640, 0o660, 0o400, 0o700] {
            fs::set_permissions(&file.path, fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(
                fs::metadata(&file.path).unwrap().permissions().mode() & 0o7777,
                mode
            );
            assert!(
                failure(&file.path).message.contains("0600"),
                "mode {mode:o}"
            );
        }
        fs::set_permissions(&file.path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load(&file.path).is_ok());
    }

    #[test]
    fn final_symlinks_and_directories_are_not_accepted() {
        let file = Fixture::new();
        let alias = file.root.join("link.json");
        std::os::unix::fs::symlink(&file.path, &alias).unwrap();
        assert_eq!(failure(&alias).code, "grant_unreadable");
        assert!(failure(&file.root).message.contains("regular file"));
        assert_eq!(failure(&file.root.join("missing")).code, "grant_unreadable");
    }

    #[test]
    fn fifo_is_rejected_without_waiting_for_a_writer() {
        use std::os::unix::ffi::OsStrExt;
        let file = Fixture::new();
        let path = file.root.join("fifo");
        let native = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = tx.send(failure(&path));
        });
        let error = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("FIFO load blocked waiting for a writer");
        assert!(error.message.contains("regular file"));
        worker.join().unwrap();
    }

    #[test]
    fn byte_limit_is_enforced_independently_of_json_and_character_limits() {
        let file = Fixture::new();
        let content = fs::read_to_string(&file.path).unwrap();
        file.write(&(content.clone() + &" ".repeat(MAX_BYTES as usize - content.len())));
        assert!(load(&file.path).is_ok());
        file.write(&(content.clone() + &" ".repeat(MAX_BYTES as usize - content.len() + 1)));
        assert!(failure(&file.path).message.contains("16 KiB"));
        file.write("not JSON");
        assert!(failure(&file.path).message.contains("valid JSON"));
        fs::write(&file.path, [0xff, 0xfe]).unwrap();
        assert!(failure(&file.path).message.contains("valid JSON"));
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn nonmac_loader_is_explicitly_unsupported() {
    match load(Path::new("ignored.json")) {
        Err(error) => assert_eq!(error.code, "unsupported_platform"),
        Ok(_) => panic!("unsupported platform loaded a grant"),
    }
}
