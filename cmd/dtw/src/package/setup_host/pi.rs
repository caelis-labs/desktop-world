use super::{action, cli::HostCli, home, shell_path, variable_set, verify, Mode};
use serde_json::{json, Value};
use std::{env, fs, io, path::Path};

const PACKAGE_NAME: &str = "@caelis-labs/desktop-world";
const REVIEW: &str = "Inspect this project's and your personal Pi packages, then register only this managed DTW package through Pi's package manager. See docs/install-pi.md.";
const VERIFY: &str = "Use /reload in Pi, then verify the live DTW tools and matching bundled Skill in a fresh session.";

pub(super) fn register(package: &Path, previous: Option<&Path>, mode: Mode) -> Value {
    if !supports_pi(package) {
        return action("Select a verified DTW package that includes the native Pi extension and matching bundled Skill, then rerun setup.");
    }
    if variable_set("PI_CODING_AGENT_DIR") {
        return action(REVIEW);
    }
    let Some(home) = home().and_then(|path| fs::canonicalize(path).ok()) else {
        return action(REVIEW);
    };
    let Some(cwd) = env::current_dir()
        .ok()
        .and_then(|path| fs::canonicalize(path).ok())
    else {
        return action(REVIEW);
    };
    if cwd
        .ancestors()
        .take_while(|dir| *dir != home)
        .any(|dir| !missing(&dir.join(".pi")) || has_standalone_skill(&dir.join(".agents/skills")))
        || !missing(&home.join(".pi/settings.json"))
    {
        return action(REVIEW);
    }
    let Some(cli) = HostCli::find("pi") else {
        return action(
            "Put the installed Pi CLI on an absolute PATH outside the workspace, then rerun setup.",
        );
    };
    pi(package, previous, &home, mode, |args| cli.run(args))
}

fn supports_pi(package: &Path) -> bool {
    use super::super::files::{json, regular};
    let Ok(manifest) = json(&package.join("package.json")) else {
        return false;
    };
    manifest["name"] == PACKAGE_NAME
        && manifest["keywords"] == serde_json::json!(["pi-package"])
        && manifest["pi"]
            == serde_json::json!({"extensions":["./pi/index.mjs"],"skills":["./skills/dtw"]})
        && regular(&package.join("pi/index.mjs"), false).is_ok()
        && regular(&package.join("skills/dtw/SKILL.md"), false).is_ok()
}

fn pi(
    package: &Path,
    previous: Option<&Path>,
    home: &Path,
    mode: Mode,
    run: impl Fn(&[&str]) -> Option<Vec<u8>>,
) -> Value {
    let status = || personal_status(home, package, previous);
    let Some(first) = status() else {
        return action(REVIEW);
    };
    if !run(&["--version"]).is_some_and(|output| {
        std::str::from_utf8(&output).is_ok_and(|version| version.trim() == "1.5.2")
    }) {
        return action("Confirm the installed Pi version and package scope, then follow the native Pi installation in docs/install-pi.md.");
    }
    if first == Registration::Exact {
        return verify(VERIFY);
    }
    if first == Registration::Previous {
        if let Some(prior) = previous {
            return action(&format!("Finish active DTW work in Pi. Confirm `pi list` still points to {}, then run `pi remove {}` followed by `pi install {}`. Use /reload and rerun setup.", shell_path(prior), shell_path(prior), shell_path(package)));
        }
    }
    if mode == Mode::Resume && first == Registration::Absent {
        return finish_pi_install(package);
    }
    if mode != Mode::New || status() != Some(Registration::Absent) {
        return action(REVIEW);
    }
    let Some(source) = package.to_str() else {
        return action(REVIEW);
    };
    let _ = run(&["install", source]);
    if status() == Some(Registration::Exact) {
        verify(VERIFY)
    } else {
        finish_pi_install(package)
    }
}

fn finish_pi_install(package: &Path) -> Value {
    action(&format!("Wait for any prior Pi install to finish before running another command. Check `pi list` in the intended scope. If it confirms this managed DTW package is still absent and no other DTW registration exists, run `pi install {}` once, then rerun setup.", shell_path(package)))
}

#[derive(Debug, PartialEq, Eq)]
enum Registration {
    Absent,
    Exact,
    Previous,
}

fn personal_status(home: &Path, package: &Path, previous: Option<&Path>) -> Option<Registration> {
    use super::super::files::{json, regular};
    let root = home.join(".pi/agent");
    for dir in [home.join(".pi"), root.clone()] {
        if !missing(&dir) {
            regular(&dir, true).ok()?;
        }
    }
    let settings_path = root.join("settings.json");
    let selected = fs::canonicalize(package).ok()?;
    let prior = previous.and_then(|path| fs::canonicalize(path).ok());
    let settings = if missing(&settings_path) {
        json!({})
    } else {
        json(&settings_path).ok()?
    };
    let settings = settings.as_object()?;
    // Explicit discovery roots can add a competing MCP adapter or standalone Skill.
    for key in ["extensions", "skills"] {
        if settings
            .get(key)
            .is_some_and(|entry| entry.as_array().is_none_or(|list| !list.is_empty()))
        {
            return None;
        }
    }
    let extensions = root.join("extensions");
    if !missing(&extensions) {
        regular(&extensions, true).ok()?;
        if fs::read_dir(&extensions).ok()?.next().is_some() {
            return None;
        }
    }
    if has_standalone_skill(&root.join("skills"))
        || has_standalone_skill(&home.join(".agents/skills"))
    {
        return None;
    }
    let mcp_path = root.join("mcp.json");
    if !missing(&mcp_path) {
        let config = json(&mcp_path).ok()?;
        if config.get("mcpServers").is_some_and(|servers| {
            servers.as_object().is_none_or(|servers| {
                ["desktop-world", "desktop_world", "dtw"]
                    .iter()
                    .any(|name| servers.contains_key(*name))
            })
        }) || !config.is_object()
        {
            return None;
        }
    }
    let mut found = Registration::Absent;
    if let Some(packages) = settings.get("packages") {
        for entry in packages.as_array()? {
            let source = entry.as_str()?;
            if let Some(npm) = source.strip_prefix("npm:") {
                // Any recognizable DTW registry name needs operator review.
                let npm = npm.to_ascii_lowercase();
                if npm.contains("desktop-world") || npm.contains("dtw") {
                    return None;
                }
                continue;
            }
            let path = Path::new(source);
            // Pi stores local sources relative to its settings directory and
            // need not preserve a leading `./`. Opaque Git/URL sources remain manual.
            if source.is_empty()
                || source.trim() != source
                || (!path.is_absolute() && source.contains([':', '~', '@']))
            {
                return None;
            }
            let registered = fs::canonicalize(root.join(path)).ok()?;
            if registered == selected || prior.as_ref() == Some(&registered) {
                if found != Registration::Absent {
                    return None;
                }
                found = if registered == selected {
                    Registration::Exact
                } else {
                    Registration::Previous
                };
            } else {
                let manifest = json(&registered.join("package.json")).ok()?;
                let name = manifest["name"].as_str()?;
                if matches!(name, PACKAGE_NAME | "desktop-world" | "dtw") {
                    return None;
                }
            }
        }
    }
    Some(found)
}

fn has_standalone_skill(root: &Path) -> bool {
    ["dtw", "desktop-world", "dtw.md", "desktop-world.md"]
        .iter()
        .any(|name| !missing(&root.join(name)))
}

fn missing(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = env::temp_dir().join(uuid::Uuid::new_v4().to_string());
            fs::create_dir_all(path.join(".pi/agent")).unwrap();
            fs::create_dir(path.join("package")).unwrap();
            Self(path)
        }

        fn settings(&self, value: Value) {
            fs::write(self.0.join(".pi/agent/settings.json"), value.to_string()).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn pi_uses_native_install_and_checks_relative_registration() {
        let fixture = Fixture::new();
        let package = fixture.0.join("package");
        let result = pi(&package, None, &fixture.0, Mode::New, |args| match args {
            ["--version"] => Some(b"1.5.2\n".to_vec()),
            ["install", source] => {
                assert_eq!(Path::new(source), package);
                fixture.settings(json!({"packages":["../../package"],"secret":"fixture token"}));
                Some(Vec::new())
            }
            _ => panic!("unexpected Pi arguments"),
        });
        assert_eq!(result["state"], "verify");
        let resumed = pi(&package, None, &fixture.0, Mode::Resume, |args| {
            assert_eq!(args, ["--version"]);
            Some(b"1.5.2\n".to_vec())
        });
        assert_eq!(resumed["state"], "verify");
    }

    #[test]
    fn pi_absent_resume_and_uncertain_install_wait_for_completion() {
        let fixture = Fixture::new();
        let package = fixture.0.join("package");
        for mode in [Mode::Resume, Mode::New] {
            let installs = std::cell::Cell::new(0);
            let result = pi(&package, None, &fixture.0, mode, |args| match args {
                ["--version"] => Some(b"1.5.2\n".to_vec()),
                ["install", source] => {
                    assert_eq!(Path::new(source), package);
                    installs.set(installs.get() + 1);
                    None
                }
                _ => panic!("unexpected Pi arguments"),
            });
            assert_eq!(installs.get(), usize::from(mode == Mode::New));
            assert_eq!(result["state"], "host_action_required");
            let next = result["next"].as_str().unwrap();
            assert!(next.contains(
                "Wait for any prior Pi install to finish before running another command"
            ));
            assert!(next.contains("Check `pi list`"));
            assert!(next.contains(&format!("pi install {}` once", shell_path(&package))));
        }
    }

    #[test]
    fn pi_hands_off_an_exact_previous_registration() {
        let fixture = Fixture::new();
        let package = fixture.0.join("package");
        let previous = fixture.0.join("previous");
        fs::create_dir(&previous).unwrap();
        fixture.settings(json!({"packages":["../../previous"]}));
        let result = pi(
            &package,
            Some(&previous),
            &fixture.0,
            Mode::Resume,
            |args| {
                assert_eq!(args, ["--version"]);
                Some(b"1.5.2\n".to_vec())
            },
        );
        assert_eq!(result["state"], "host_action_required");
        let next = result["next"].as_str().unwrap();
        assert!(next.contains(&format!("pi remove {}", shell_path(&previous))));
        assert!(next.contains(&format!("pi install {}", shell_path(&package))));
    }

    #[test]
    fn pi_refuses_duplicate_and_filtered_entries_before_install() {
        let fixture = Fixture::new();
        let package = fixture.0.join("package");
        for packages in [
            json!(["../../package", "../../package"]),
            json!([{"source":"../../package","skills":[]}]),
            json!(["npm:@caelis-labs/desktop-world"]),
        ] {
            fixture.settings(json!({"packages":packages}));
            assert_eq!(
                pi(&package, None, &fixture.0, Mode::Resume, |_| panic!(
                    "Pi must not be called"
                ))["state"],
                "host_action_required"
            );
        }
    }
}
