use super::{action, package_path, variable_set, verify, Mode};
use serde_json::Value;

const REVIEW_PROFILE: &str = "Back up the active DSH Desktop profile's cordis.patch.yml (default ~/.dsh/profiles/desktop/cordis.patch.yml). Review existing DTW rows, then set the name of the single inserted desktop-world-plugin row to install.cordis_url. Save your work and reopen DSH normally.";
const VERIFY: &str = "In a fresh DSH Desktop context, verify the live DTW connection and bundled Skill resolve to this managed package. If DSH still uses the old package, save your work and reopen DSH normally.";

pub(super) fn register(install: &Value, previous: Option<&Value>, mode: Mode) -> Value {
    if [
        "DSH_HOME",
        "DSH_PROFILE",
        "DSH_PROFILE_DIR",
        "DSH_CONFIG_DIR",
    ]
    .iter()
    .any(|name| variable_set(name))
    {
        return action(REVIEW_PROFILE);
    }
    let Some(url) = cordis_url(install) else {
        return action(
            "Reimport a complete verified package with its Cordis adapter, then rerun setup.",
        );
    };
    let previous_url = match previous {
        Some(recipe) => match cordis_url(recipe) {
            Some(url) => Some(url),
            None => return action(REVIEW_PROFILE),
        },
        None => None,
    };
    if let Some(home) = super::home() {
        #[cfg(unix)]
        if let Some(result) = edit_profile(&home, &url, previous_url.as_deref(), mode) {
            return result;
        }
        #[cfg(windows)]
        if exact_default_patch(&home, &url).unwrap_or(false) {
            return verify(VERIFY);
        }
    }
    let _ = (url, previous_url, mode);
    action(REVIEW_PROFILE)
}

fn cordis_url(recipe: &Value) -> Option<String> {
    let path = package_path(recipe).ok()?.join("cordis/index.mjs");
    let url = super::super::files::file_url(&path).ok()?;
    (recipe["cordis_url"] == url && scalar(&url) && !url.contains('\'')).then_some(url)
}

#[derive(Debug, PartialEq, Eq)]
enum Edit {
    Exact,
    Write(String),
}

// Recognize only plain insert blocks with two-field rows and ordinary comments.
// Reject unsupported YAML before classifying a missing row as safe to insert.
fn plan_edit(text: &str, url: &str, previous: Option<&str>, mode: Mode) -> Option<Edit> {
    use std::collections::HashSet;
    let mut offset = 0;
    let mut ids = HashSet::new();
    let mut rows = None;
    let mut pending_id = None;
    let mut found = None;
    let mut empty_list = None;
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };

    for line in text.split_inclusive('\n') {
        let without_ending = line.strip_suffix(newline).unwrap_or(line);
        if without_ending.contains(['\r', '\t', '\n']) {
            return None;
        }
        let body = uncomment(without_ending);
        let line_offset = offset;
        offset += line.len();
        if body.is_empty() {
            continue;
        }
        if body == "[]" && rows.is_none() && empty_list.is_none() {
            empty_list = Some(line_offset..line_offset + body.len());
            continue;
        }
        if empty_list.is_some() {
            return None;
        }
        if body == "- insert:" && pending_id.is_none() && rows != Some(0) {
            rows = Some(0);
            continue;
        }
        let row_count = rows.as_mut()?;
        if let Some(id) = body
            .strip_prefix("    - id: ")
            .filter(|_| pending_id.is_none())
        {
            let id = unquote(id)?;
            if !ids.insert(id) || legacy_id(id) {
                return None;
            }
            pending_id = Some(id);
            continue;
        }
        let name = body.strip_prefix("      name: ")?;
        let name_value = unquote(name)?;
        let id = pending_id.take()?;
        *row_count += 1;
        if id == "desktop-world-plugin" {
            found = Some((
                name_value,
                line_offset + body.len() - name.len()..line_offset + body.len(),
            ));
        } else if name_value.contains("desktop-world") || name_value.ends_with("/cordis/index.mjs")
        {
            return None;
        }
    }
    if pending_id.is_some() || rows == Some(0) {
        return None;
    }

    if let Some((existing, span)) = found {
        if existing == url {
            return Some(Edit::Exact);
        }
        if mode == Mode::Current || Some(existing) != previous {
            return None;
        }
        let mut patched = text.to_owned();
        patched.replace_range(span, &format!("'{url}'"));
        return Some(Edit::Write(patched));
    }
    if mode == Mode::Current || previous.is_some() {
        return None;
    }
    let row =
        format!("- insert:{newline}    - id: desktop-world-plugin{newline}      name: '{url}'");
    if let Some(span) = empty_list {
        let mut patched = text.to_owned();
        patched.replace_range(span, &row);
        return Some(Edit::Write(patched));
    }
    let separator = if text.is_empty() || text.ends_with('\n') {
        ""
    } else {
        newline
    };
    Some(Edit::Write(format!("{text}{separator}{row}{newline}")))
}

fn uncomment(line: &str) -> &str {
    if line.trim_start().starts_with('#') {
        return "";
    }
    let comment = line.find(" #").unwrap_or(line.len());
    line[..comment].trim_end()
}

fn unquote(value: &str) -> Option<&str> {
    let value = if let Some(quoted) = value.strip_prefix('\'') {
        quoted.strip_suffix('\'')?
    } else if let Some(quoted) = value.strip_prefix('"') {
        quoted.strip_suffix('"')?
    } else {
        if !value.starts_with(|c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '.')) {
            return None;
        }
        value
    };
    scalar(value).then_some(value)
}

fn scalar(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@-/.:+%~()".contains(&b))
}

fn legacy_id(id: &str) -> bool {
    matches!(
        id,
        "desktop-world" | "dtw" | "mcp-desktop-world" | "mcp-dtw"
    )
}

#[cfg(windows)]
fn exact_default_patch(home: &std::path::Path, url: &str) -> Option<bool> {
    use std::{fs, io::Read};
    let mut path = fs::canonicalize(home).ok()?;
    for component in [".dsh", "profiles", "desktop"] {
        path.push(component);
        super::super::files::regular(&path, true).ok()?;
    }
    path.push("cordis.patch.yml");
    super::super::files::regular(&path, false).ok()?;
    let mut content = String::new();
    fs::File::open(path)
        .ok()?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut content)
        .ok()?;
    Some(
        content.len() <= 1024 * 1024
            && plan_edit(&content, url, None, Mode::Current) == Some(Edit::Exact),
    )
}

#[cfg(unix)]
fn edit_profile(
    home: &std::path::Path,
    url: &str,
    previous: Option<&str>,
    mode: Mode,
) -> Option<Value> {
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    };

    let uid = unsafe { libc::geteuid() };
    let mut dir = fs::canonicalize(home).ok()?;
    for component in [".dsh", "profiles", "desktop"] {
        dir.push(component);
        let metadata = fs::symlink_metadata(&dir).ok()?;
        if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
            return None;
        }
    }
    // Coordinate this adapter across stores without adding a lock file to the profile.
    let directory_lock = File::open(&dir).ok()?;
    directory_lock.try_lock().ok()?;
    let path = dir.join("cordis.patch.yml");
    let read_patch = || {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
            .ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file()
            || metadata.uid() != uid
            || metadata.nlink() != 1
            || metadata.mode() & 0o022 != 0
        {
            return None;
        }
        let mut content = String::new();
        (&mut file)
            .take(1024 * 1024 + 1)
            .read_to_string(&mut content)
            .ok()?;
        (content.len() <= 1024 * 1024).then_some((content, metadata))
    };
    let (original, metadata) = read_patch()?;
    let Edit::Write(updated) = plan_edit(&original, url, previous, mode)? else {
        return Some(verify(VERIFY));
    };
    let id = uuid::Uuid::new_v4();
    let backup = dir.join(format!("cordis.patch.yml.dtw-{id}.bak"));
    let temporary = dir.join(format!(".cordis.patch.yml.dtw-{id}.tmp"));
    let write_new = |path: &std::path::Path, content: &str, permissions: u32| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .ok()?;
        let written = file
            .write_all(content.as_bytes())
            .and_then(|_| file.set_permissions(fs::Permissions::from_mode(permissions)))
            .and_then(|_| file.sync_all());
        if written.is_err() {
            let _ = fs::remove_file(path);
        }
        written.ok()
    };
    write_new(&backup, &original, 0o600)?;
    // A replacement must never broaden access if the original had a private ACL.
    write_new(&temporary, &updated, metadata.mode() & 0o700)?;
    let unchanged = read_patch().is_some_and(|(content, current)| {
        content == original
            && current.dev() == metadata.dev()
            && current.ino() == metadata.ino()
            && current.mode() == metadata.mode()
    });
    if !unchanged || fs::rename(&temporary, &path).is_err() {
        let _ = fs::remove_file(&temporary);
        return None;
    }
    Some(serde_json::json!({
        "state":"restart_required",
        "next":"Save your work, quit and reopen DSH Desktop normally, then rerun setup to continue with live verification.",
        "backup":backup,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW: &str = "file:///new/cordis/index.mjs";
    const OLD: &str = "file:///old/cordis/index.mjs";

    #[test]
    fn replaces_only_prior_exact_url_and_resumes() {
        let original = format!("# private profile\r\n- insert:\r\n    - id: desktop-world-plugin\r\n      name: '{OLD}' # old DTW\r\n    - id: other\r\n      name: '@example/other'\r\n");
        let expected = original.replace(OLD, NEW);
        assert_eq!(
            plan_edit(&original, NEW, Some(OLD), Mode::New),
            Some(Edit::Write(expected.clone()))
        );
        assert_eq!(
            plan_edit(&expected, NEW, Some(OLD), Mode::Resume),
            Some(Edit::Exact)
        );
        assert_eq!(plan_edit(&original, NEW, None, Mode::New), None);
        assert_eq!(plan_edit(&original, NEW, Some(OLD), Mode::Current), None);
    }

    #[test]
    fn appends_documented_row_without_rewriting_other_lines() {
        let original = "- insert:\n    - id: other\n      name: '@example/other' # retain";
        let Some(Edit::Write(updated)) = plan_edit(original, NEW, None, Mode::New) else {
            panic!("expected one insertion")
        };
        assert!(updated.starts_with(original));
        assert_eq!(
            plan_edit(&updated, NEW, None, Mode::Resume),
            Some(Edit::Exact)
        );
    }

    #[test]
    fn rejects_legacy_duplicates_overrides_and_unsupported_yaml() {
        for text in [
            "- insert:\n    - id: mcp-desktop-world\n      name: '@deepseek-ai/dsh-mcp-client'\n".to_owned(),
            "- insert: [{ id: desktop-world-plugin }]\n".to_owned(),
            format!("- id: desktop-world-plugin\n  name: '{NEW}'\n"),
            format!("- insert:\n    - id: desktop-world-plugin\n      name: '{NEW}'\n    - id: desktop-world-plugin\n      name: '{OLD}'\n"),
        ] {
            assert_eq!(plan_edit(&text, NEW, Some(OLD), Mode::Resume), None);
        }
    }

    #[cfg(unix)]
    #[test]
    fn writes_private_backup_in_disposable_profile_once() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let home = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let profile = home.join(".dsh/profiles/desktop");
        fs::create_dir_all(&profile).unwrap();
        let path = profile.join("cordis.patch.yml");
        fs::write(&path, "[]\n").unwrap();
        let first = edit_profile(&home, NEW, None, Mode::New).unwrap();
        assert_eq!(first["state"], "restart_required");
        let backup = first["backup"].as_str().unwrap();
        assert_eq!(fs::read_to_string(backup).unwrap(), "[]\n");
        assert_eq!(
            fs::metadata(backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            edit_profile(&home, NEW, None, Mode::Resume).unwrap()["state"],
            "verify"
        );
        assert_eq!(fs::read_dir(profile).unwrap().count(), 2);
        fs::remove_dir_all(home).unwrap();
    }
}
