use super::{action, Mode};
use serde_json::Value;
use std::path::Path;

pub(super) fn register(
    package: &Path,
    install: &Value,
    previous: Option<&Value>,
    mode: Mode,
) -> Value {
    #[cfg(unix)]
    return supported::register(package, install, previous, mode);
    #[cfg(not(unix))]
    {
        let _ = (package, install, previous, mode);
        action("Follow docs/install-opencode.md to configure the intended OpenCode scope and reload it.")
    }
}

#[cfg(unix)]
mod supported {
    use super::super::{cli::HostCli, home, package_path, variable_set, verify};
    use super::{action, Mode, Path, Value};
    use serde_json::json;
    use std::{
        collections::BTreeMap,
        env,
        fs::{self, DirBuilder, Metadata, OpenOptions},
        io::{self, Read, Write},
        ops::Range,
        os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        path::PathBuf,
    };

    const FILE: &str = "opencode.jsonc";
    const MAX_CONFIG: usize = 1024 * 1024;
    const CONFIG_NAMES: [&str; 4] = [FILE, "opencode.json", "config.json", "config.jsonc"];
    const SCOPE_OVERRIDES: &[&str] = &[
        "XDG_CONFIG_HOME",
        "OPENCODE_CONFIG",
        "OPENCODE_CONFIG_CONTENT",
        "OPENCODE_CONFIG_DIR",
        "OPENCODE_CLI_CONFIG_CONTENT",
        "OPENCODE_CONFIG_PROJECT_DISABLE",
        "OPENCODE_DISABLE_PROJECT_CONFIG",
        "OPENCODE_TEST_HOME",
        "OPENCODE_FLAGS",
        "OPENCODE_CLAUDE_PATH",
        "OPENCODE_DRIVE",
        "OPENCODE_WORKTREE_BASE",
        "OPENCODE_WORKTREE_PATH",
    ];
    const REVIEW_SCOPE: &str = "Confirm OpenCode 2.3.0's configuration sources in the intended workspace. Automatic setup requires the default user opencode.jsonc and no competing configuration source. Use docs/install-opencode.md for this scope.";
    const REVIEW_PAIR: &str = "Review the exact DTW MCP and Skill entries in the default OpenCode opencode.jsonc. For an upgrade, both entries must match the previous managed package. Follow docs/install-opencode.md to resolve conflicting or unsupported entries.";
    const VERIFY: &str = "Start a new OpenCode session and verify the live DTW tools and matching bundled Skill. If OpenCode is still using the prior configuration, run `opencode reload` before starting a new session.";
    const RELOAD: &str = "The OpenCode configuration was updated. Run `opencode reload` in the intended OpenCode installation, then start a new session and verify the live DTW tools and bundled Skill.";

    pub(super) fn register(
        package: &Path,
        install: &Value,
        previous: Option<&Value>,
        mode: Mode,
    ) -> Value {
        if SCOPE_OVERRIDES.iter().any(|key| variable_set(key)) {
            return action(REVIEW_SCOPE);
        }
        let Some(home) = home().and_then(|path| path.canonicalize().ok()) else {
            return action(REVIEW_SCOPE);
        };
        let Ok(cwd) = env::current_dir().and_then(|path| path.canonicalize()) else {
            return action(REVIEW_SCOPE);
        };
        let Some(cli) = HostCli::find("opencode") else {
            return action("Put the installed OpenCode 2.3.0 binary on an absolute PATH outside the workspace, then rerun setup.");
        };
        let Some(desired) = Pair::managed(package, install) else {
            return action(
                "Reimport the complete verified DTW package before configuring OpenCode.",
            );
        };
        let previous = match previous {
            Some(recipe) => {
                let Some(pair) = package_path(recipe)
                    .ok()
                    .and_then(|path| Pair::managed(path, recipe))
                else {
                    return action(REVIEW_PAIR);
                };
                Some(pair)
            }
            None => None,
        };
        register_default(&home, &cwd, &desired, previous.as_ref(), mode, |args| {
            cli.run(args)
        })
    }

    fn register_default(
        home: &Path,
        cwd: &Path,
        desired: &Pair,
        previous: Option<&Pair>,
        mode: Mode,
        run: impl Fn(&[&str]) -> Option<Vec<u8>>,
    ) -> Value {
        let dir = home.join(".config/opencode");
        let uid = unsafe { libc::geteuid() };
        if !default_scope(home, cwd)
            || [home.join(".config"), dir.clone()].iter().any(|path| {
                match fs::symlink_metadata(path) {
                    Ok(attr) => !attr.is_dir() || !owned(&attr, uid),
                    Err(error) => error.kind() != io::ErrorKind::NotFound,
                }
            })
        {
            return action(REVIEW_SCOPE);
        }
        if (mode == Mode::Current || previous.is_some()) && absent(&dir.join(FILE)) {
            return action(REVIEW_PAIR);
        }
        if run(&["--version"]).as_deref().and_then(output_line) != Some("opencode v2.3.0") {
            return action("Confirm the installed OpenCode version. Automatic registration supports the documented OpenCode 2.3.0 host. Use docs/install-opencode.md for manual registration in other versions or forks.");
        }
        if !run(&["debug", "paths"])
            .as_deref()
            .is_some_and(|output| paths_match(output, home, &dir))
        {
            return action(REVIEW_SCOPE);
        }
        match update_config(home, cwd, &dir, desired, previous, mode) {
            Ok(Change::Exact) => verify(VERIFY),
            Ok(Change::Written(backup)) => {
                let mut result = if !default_scope(home, cwd) {
                    action(REVIEW_SCOPE)
                } else if run(&["reload"]).as_deref().and_then(output_line)
                    != Some("Configuration reloaded")
                {
                    action(RELOAD)
                } else if read_config(&dir.join(FILE), uid).is_some_and(|(updated, _)| {
                    plan_edit(&updated, desired, None, Mode::Current) == Some(Edit::Exact)
                }) {
                    verify(VERIFY)
                } else {
                    action(REVIEW_PAIR)
                };
                if let Some(backup) = backup {
                    result["backup"] = json!(backup);
                }
                result
            }
            Err(next) => action(next),
        }
    }

    fn output_line(bytes: &[u8]) -> Option<&str> {
        Some(std::str::from_utf8(bytes).ok()?.trim())
    }

    fn paths_match(output: &[u8], home: &Path, config: &Path) -> bool {
        let Some(output) = output_line(output) else {
            return false;
        };
        for (label, expected) in [("home", home), ("config", config)] {
            let mut lines = output.lines().filter_map(|line| {
                let (key, path) = line.split_once(char::is_whitespace)?;
                (key == label).then(|| Path::new(path.trim_start()))
            });
            if lines.next() != Some(expected) || lines.next().is_some() {
                return false;
            }
        }
        true
    }

    fn absent(path: &Path) -> bool {
        matches!(fs::symlink_metadata(path), Err(error) if error.kind() == io::ErrorKind::NotFound)
    }

    fn default_scope(home: &Path, cwd: &Path) -> bool {
        let global = home.join(".config/opencode");
        if !CONFIG_NAMES[1..]
            .iter()
            .all(|name| absent(&global.join(name)))
            || !CONFIG_NAMES
                .iter()
                .all(|name| absent(&home.join(".opencode").join(name)))
        {
            return false;
        }
        // A project source can override the user configuration. Do not ask the
        // CLI to load workspace files or start a service just to inspect them.
        cwd.ancestors().chain(std::iter::once(home)).all(|dir| {
            CONFIG_NAMES[..2].iter().all(|name| absent(&dir.join(name)))
                && absent(&dir.join(".mcp.json"))
                && (dir == home || absent(&dir.join(".opencode")))
        })
    }

    struct Pair {
        name: &'static str,
        executable: String,
        skill: String,
    }

    impl Pair {
        fn managed(package: &Path, recipe: &Value) -> Option<Self> {
            let build = crate::package::files::json(&package.join("BUILD.json")).ok()?;
            let name = match build["package_store_schema"].as_u64()? {
                1 => "desktop-world",
                2 => "dtw",
                _ => return None,
            };
            let executable = package.join(build["binary"]["path"].as_str()?);
            let executable = executable.to_str()?;
            let skill = package.join("skills").join(name);
            let skill = skill.to_str()?;
            (recipe["executable"] == executable
                && ![executable, skill].iter().any(|path| dynamic_path(path)))
            .then_some(Self {
                name,
                executable: executable.into(),
                skill: skill.into(),
            })
        }

        fn server(&self) -> Value {
            json!({"type":"local", "command":[self.executable, "mcp"]})
        }

        fn document(&self) -> String {
            let server = self.server();
            let name = self.name;
            let config = json!({"mcp":{"servers":{name:server}}, "skills":[self.skill]});
            format!("{}\n", serde_json::to_string_pretty(&config).unwrap())
        }
    }

    enum Change {
        Exact,
        Written(Option<PathBuf>),
    }

    fn update_config(
        home: &Path,
        cwd: &Path,
        dir: &Path,
        desired: &Pair,
        previous: Option<&Pair>,
        mode: Mode,
    ) -> Result<Change, &'static str> {
        const WRITE_FAILED: &str = "OpenCode's default configuration could not be safely updated. Review the file and any private DTW backup, then rerun setup.";
        let uid = unsafe { libc::geteuid() };
        let path = dir.join(FILE);
        let may_create = mode != Mode::Current && previous.is_none();
        for directory in [home.join(".config"), dir.to_path_buf()] {
            if absent(&directory) && may_create {
                match DirBuilder::new().mode(0o700).create(&directory) {
                    Ok(()) => (),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                    Err(_) => return Err(WRITE_FAILED),
                }
            }
            let metadata = fs::symlink_metadata(&directory).map_err(|_| REVIEW_SCOPE)?;
            if !metadata.is_dir() || !owned(&metadata, uid) {
                return Err(REVIEW_SCOPE);
            }
        }
        // Coordinate setup across separate stores and profiles for the same user.
        let directory_lock = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(dir)
            .map_err(|_| WRITE_FAILED)?;
        if !owned(&directory_lock.metadata().map_err(|_| WRITE_FAILED)?, uid) {
            return Err(REVIEW_SCOPE);
        }
        directory_lock.try_lock().map_err(|_| WRITE_FAILED)?;
        if !default_scope(home, cwd) {
            return Err(REVIEW_SCOPE);
        }
        let original = if absent(&path) {
            if !may_create {
                return Err(REVIEW_PAIR);
            }
            None
        } else {
            Some(read_config(&path, uid).ok_or(REVIEW_SCOPE)?)
        };
        let updated = match &original {
            Some((text, _)) => match plan_edit(text, desired, previous, mode).ok_or(REVIEW_PAIR)? {
                Edit::Exact => return Ok(Change::Exact),
                Edit::Write(updated) => updated,
            },
            None => desired.document(),
        };
        let id = uuid::Uuid::new_v4();
        let temporary = dir.join(format!(".{FILE}.dtw-{id}.tmp"));
        // A new inode may lose the original's restrictive ACL, so retain
        // only the existing owner's read and write permissions.
        let permissions = original
            .as_ref()
            .map_or(0o600, |(_, attr)| attr.mode() & 0o600);
        write_new(&temporary, &updated, permissions).map_err(|_| WRITE_FAILED)?;
        let result = (|| {
            if !default_scope(home, cwd) {
                return Err(REVIEW_SCOPE);
            }
            let Some((text, attr)) = original else {
                fs::hard_link(&temporary, &path).map_err(|_| WRITE_FAILED)?;
                return Ok(Change::Written(None));
            };
            if !same_config(&path, uid, &text, &attr) {
                return Err(WRITE_FAILED);
            }
            let backup = dir.join(format!("{FILE}.dtw-{id}.bak"));
            write_new(&backup, &text, 0o600).map_err(|_| WRITE_FAILED)?;
            if !same_config(&path, uid, &text, &attr) {
                return Err(WRITE_FAILED);
            }
            fs::rename(&temporary, &path).map_err(|_| WRITE_FAILED)?;
            Ok(Change::Written(Some(backup)))
        })();
        let _ = fs::remove_file(&temporary);
        result
    }

    fn owned(attr: &Metadata, uid: u32) -> bool {
        attr.uid() == uid && attr.mode() & 0o022 == 0
    }

    fn read_config(path: &Path, uid: u32) -> Option<(String, Metadata)> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .ok()?;
        let attr = file.metadata().ok()?;
        if !attr.is_file() || !owned(&attr, uid) || attr.nlink() != 1 {
            return None;
        }
        let mut text = String::new();
        file.take((MAX_CONFIG + 1) as u64)
            .read_to_string(&mut text)
            .ok()?;
        (text.len() <= MAX_CONFIG).then_some((text, attr))
    }

    fn same_config(path: &Path, uid: u32, text: &str, attr: &Metadata) -> bool {
        read_config(path, uid).is_some_and(|(current, metadata)| {
            current == text
                && metadata.dev() == attr.dev()
                && metadata.ino() == attr.ino()
                && metadata.mode() == attr.mode()
        })
    }

    fn write_new(path: &Path, text: &str, permissions: u32) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        let result = file
            .write_all(text.as_bytes())
            .and_then(|_| file.set_permissions(fs::Permissions::from_mode(permissions)))
            .and_then(|_| file.sync_all());
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
        result
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Edit {
        Exact,
        Write(String),
    }

    fn plan_edit(text: &str, desired: &Pair, previous: Option<&Pair>, mode: Mode) -> Option<Edit> {
        let doc = JsoncSpans::new(text)?;
        let root = doc.fields(0)?;
        // These forms belong to other config schemas and can hide another DTW registration.
        if root.contains_key("mcpServers") {
            return None;
        }
        let mcp_at = root.get("mcp").copied();
        let servers_at = match mcp_at {
            Some(at) => {
                let fields = doc.fields(at)?;
                if fields.keys().any(|key| key != "servers") {
                    return None;
                }
                fields.get("servers").copied()
            }
            None => None,
        };
        let servers = match servers_at {
            Some(at) => doc.fields(at)?,
            None => BTreeMap::new(),
        };
        let skills_at = root.get("skills").copied();
        let mut related_skills = Vec::new();
        if let Some(at) = skills_at {
            for item in doc.elements(at)? {
                let skill = doc.string(item)?;
                if dynamic_path(&skill) {
                    return None;
                }
                if related_path(
                    skill.strip_suffix("/SKILL.md").unwrap_or(&skill),
                    &["dtw", "desktop-world"],
                ) {
                    related_skills.push((item, skill));
                }
            }
        }
        let mut related_servers = Vec::new();
        for (name, at) in servers {
            if name.eq_ignore_ascii_case("dtw") || name.eq_ignore_ascii_case("desktop-world") {
                related_servers.push((name, at));
                continue;
            }
            if let Some(&command) = doc.fields(at)?.get("command") {
                for arg in doc.elements(command)? {
                    let arg = doc.string(arg)?;
                    if related_path(&arg, &["dtw", "dtw.exe"]) || dynamic_path(&arg) {
                        return None;
                    }
                }
            }
        }
        match (related_servers.as_slice(), related_skills.as_slice()) {
            ([(name, server)], [(skill_at, skill)]) => {
                let exact = |pair: &Pair| {
                    name == pair.name
                        && skill == &pair.skill
                        && doc.value(*server) == Some(pair.server())
                };
                let fields = doc.fields(*server)?;
                if exact(desired) {
                    return Some(Edit::Exact);
                }
                if mode == Mode::Current || !exact(previous?) {
                    return None;
                }
                let command = doc.elements(*fields.get("command")?)?;
                let mut edits = vec![
                    doc.replace(command[0], &desired.executable),
                    doc.replace(*skill_at, &desired.skill),
                ];
                if name != desired.name {
                    edits.push(doc.replace(*server - 2, desired.name));
                }
                Some(Edit::Write(apply(text, edits)))
            }
            ([], []) if mode != Mode::Current && previous.is_none() => {
                let server = format!("{}: {}", quote(desired.name), desired.server());
                let skill = quote(&desired.skill);
                let mut edits = Vec::new();
                let mut root_fields = Vec::new();
                if let Some(at) = servers_at {
                    edits.push(doc.insert(at, &server));
                } else if let Some(at) = mcp_at {
                    edits.push(doc.insert(at, &format!("\"servers\": {{{server}}}")));
                } else {
                    root_fields.push(format!("\"mcp\": {{\"servers\": {{{server}}}}}"));
                }
                if let Some(at) = skills_at {
                    edits.push(doc.insert(at, &skill));
                } else {
                    root_fields.push(format!("\"skills\": [{skill}]"));
                }
                if !root_fields.is_empty() {
                    edits.push(doc.insert(0, &root_fields.join(",\n")));
                }
                Some(Edit::Write(apply(text, edits)))
            }
            _ => None,
        }
    }

    fn dynamic_path(path: &str) -> bool {
        path.contains("{env:") || path.contains("{file:")
    }

    fn related_path(path: &str, names: &[&str]) -> bool {
        let last = path
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default();
        names.iter().any(|name| last.eq_ignore_ascii_case(name))
    }

    fn quote(value: &str) -> String {
        serde_json::to_string(value).unwrap()
    }

    fn apply(text: &str, mut edits: Vec<(Range<usize>, String)>) -> String {
        edits.sort_unstable_by_key(|(span, _)| std::cmp::Reverse(span.start));
        let mut updated = text.to_owned();
        for (span, replacement) in edits {
            updated.replace_range(span, &replacement);
        }
        updated
    }

    // Keep original byte offsets. Only comments and trailing commas are masked
    // for serde_json validation; the resulting JSON is never used for writes.
    struct JsoncSpans {
        json: Vec<u8>,
        tokens: Vec<Range<usize>>,
    }

    impl JsoncSpans {
        fn new(text: &str) -> Option<Self> {
            let bytes = text.as_bytes();
            let mut json = bytes.to_vec();
            let mut tokens: Vec<Range<usize>> = Vec::new();
            let mut i = 0;
            while i < bytes.len() {
                let start = i;
                i += 1;
                match bytes[start] {
                    b' ' | b'\n' | b'\r' | b'\t' => continue,
                    b'/' if bytes.get(i) == Some(&b'/') => {
                        i += bytes[i..]
                            .iter()
                            .position(|b| matches!(b, b'\r' | b'\n'))
                            .unwrap_or(bytes.len() - i);
                        json[start..i].fill(b' ');
                        continue;
                    }
                    b'/' if bytes.get(i) == Some(&b'*') => {
                        i += 1;
                        i += bytes[i..].windows(2).position(|pair| pair == b"*/")? + 2;
                        json[start..i].fill(b' ');
                        continue;
                    }
                    b'"' => loop {
                        match *bytes.get(i)? {
                            b'\\' => i += 2,
                            b'"' => {
                                i += 1;
                                break;
                            }
                            _ => i += 1,
                        }
                    },
                    b'}' | b']' => {
                        if tokens.len() > 1 {
                            let comma = &tokens[tokens.len() - 1];
                            let preceding = bytes[tokens[tokens.len() - 2].start];
                            if bytes[comma.start] == b',' && !b"{[,".contains(&preceding) {
                                json[comma.start] = b' ';
                                tokens.pop();
                            }
                        }
                    }
                    b'{' | b'[' | b':' | b',' => (),
                    _ => {
                        while bytes
                            .get(i)
                            .is_some_and(|b| !b" \r\n\t{}[]:,/\"".contains(b))
                        {
                            i += 1;
                        }
                    }
                }
                tokens.push(start..i);
            }
            serde_json::from_slice::<Value>(&json).ok()?;
            Some(Self { json, tokens })
        }

        fn kind(&self, i: usize) -> u8 {
            self.json[self.tokens[i].start]
        }

        fn end(&self, at: usize) -> usize {
            if !matches!(self.kind(at), b'{' | b'[') {
                return at + 1;
            }
            let mut depth = 1;
            for i in at + 1..self.tokens.len() {
                match self.kind(i) {
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => depth -= 1,
                    _ => (),
                }
                if depth == 0 {
                    return i + 1;
                }
            }
            unreachable!("validated JSON value")
        }

        fn value(&self, at: usize) -> Option<Value> {
            serde_json::from_slice(
                &self.json[self.tokens[at].start..self.tokens[self.end(at) - 1].end],
            )
            .ok()
        }

        fn string(&self, at: usize) -> Option<String> {
            serde_json::from_slice(&self.json[self.tokens[at].clone()]).ok()
        }

        fn fields(&self, at: usize) -> Option<BTreeMap<String, usize>> {
            if self.kind(at) != b'{' {
                return None;
            }
            let mut fields = BTreeMap::new();
            let mut i = at + 1;
            while self.kind(i) != b'}' {
                if fields.insert(self.string(i)?, i + 2).is_some() {
                    return None;
                }
                i = self.end(i + 2);
                if self.kind(i) == b',' {
                    i += 1;
                }
            }
            Some(fields)
        }

        fn elements(&self, at: usize) -> Option<Vec<usize>> {
            if self.kind(at) != b'[' {
                return None;
            }
            let mut items = Vec::new();
            let mut i = at + 1;
            while self.kind(i) != b']' {
                items.push(i);
                i = self.end(i);
                if self.kind(i) == b',' {
                    i += 1;
                }
            }
            Some(items)
        }

        fn replace(&self, at: usize, value: &str) -> (Range<usize>, String) {
            (self.tokens[at].clone(), quote(value))
        }

        fn insert(&self, at: usize, contents: &str) -> (Range<usize>, String) {
            let offset = self.tokens[at].end;
            let separator = if matches!(self.kind(at + 1), b'}' | b']') {
                ""
            } else {
                ","
            };
            (offset..offset, format!("\n{contents}{separator}\n"))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::{cell::RefCell, os::unix::fs::symlink};

        fn pair(name: &'static str, version: &str) -> Pair {
            Pair {
                name,
                executable: format!("/managed/{version}/bin/dtw"),
                skill: format!("/managed/{version}/skills/{name}"),
            }
        }

        struct TestHome(PathBuf);

        impl TestHome {
            fn new() -> Self {
                let path = env::temp_dir().join(uuid::Uuid::new_v4().to_string());
                DirBuilder::new().mode(0o700).create(&path).unwrap();
                Self(path.canonicalize().unwrap())
            }

            fn file(&self) -> PathBuf {
                self.0.join(".config/opencode").join(FILE)
            }

            fn run(&self, args: &[&str]) -> Vec<u8> {
                match args {
                    ["--version"] => b"opencode v2.3.0\n".to_vec(),
                    ["debug", "paths"] => format!(
                        "home       {}\nconfig     {}\n",
                        self.0.display(),
                        self.file().parent().unwrap().display()
                    )
                    .into_bytes(),
                    ["reload"] => b"Configuration reloaded\n".to_vec(),
                    _ => panic!("unexpected host command"),
                }
            }
        }

        impl Drop for TestHome {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn inserts_scoped_fragments_without_reformatting_existing_jsonc() {
            let desired = pair("dtw", "new");
            let text = r#"{
  // retain provider and credentials
  "provider": {"url": "https://example.org/*literal*/", "key": "private"},
  "mcp": {"servers": {/* retain other server */ "other": {"type":"remote"},}},
  "skills": ["/other/skill", /* retain nearby comment */],
}
"#;
            let Some(Edit::Write(updated)) = plan_edit(text, &desired, None, Mode::New) else {
                panic!("expected scoped insertions")
            };
            let expected = text
                .replace(
                    "{/* retain other server */",
                    &format!(
                        "{{\n\"dtw\": {},\n/* retain other server */",
                        desired.server()
                    ),
                )
                .replace(
                    "[\"/other/skill\"",
                    &format!("[\n{},\n\"/other/skill\"", quote(&desired.skill)),
                );
            assert_eq!(updated, expected);
            assert_eq!(
                plan_edit(&updated, &desired, None, Mode::Resume),
                Some(Edit::Exact)
            );
        }

        #[test]
        fn upgrades_only_exact_old_pair_and_preserves_comments_in_both_entries() {
            let old = pair("desktop-world", "old");
            let new = pair("dtw", "new");
            let text = format!(
                r#"{{"mcp":{{"servers":{{"desktop-world" /* name */: {{
                "type":"local",
                "command":[{} /* executable */, "mcp",],
            }}}}}},"skills":["/other/skill", {} /* skill */,]}}"#,
                quote(&old.executable),
                quote(&old.skill)
            );
            let expected = text
                .replace("\"desktop-world\"", "\"dtw\"")
                .replace(&old.executable, &new.executable)
                .replace(&old.skill, &new.skill);
            assert_eq!(
                plan_edit(&text, &new, Some(&old), Mode::New),
                Some(Edit::Write(expected.clone()))
            );
            assert_eq!(
                plan_edit(&expected, &new, Some(&old), Mode::Resume),
                Some(Edit::Exact)
            );
            assert_eq!(plan_edit(&text, &new, None, Mode::New), None);
            assert_eq!(plan_edit(&text, &new, Some(&old), Mode::Current), None);
        }

        #[test]
        fn refuses_conflicts_duplicates_and_other_config_shapes() {
            let new = pair("dtw", "new");
            let duplicate_key = new
                .document()
                .replace("\"type\":", "\"type\": \"remote\", \"type\":");
            let duplicate_skill = new
                .document()
                .replace(&quote(&new.skill), &format!("{0}, {0}", quote(&new.skill)));
            for text in [
                duplicate_key.as_str(),
                duplicate_skill.as_str(),
                r#"{"mcpServers":{}}"#,
                r#"{"mcp": {"dtw":{"type":"local"}}}"#,
                r#"{"mcp": {"servers": {"custom":{"command":["/elsewhere/bin/dtw","mcp"]}}}}"#,
                r#"{"mcp": {}, "\u006dcp": {}}"#,
                r#"{"skills": {"paths":[]}}"#,
                r#"{"skills": ["/other/desktop-world"]}"#,
                r#"{"skills": ["{env:EXTERNAL_SKILL}"]}"#,
                r#"{"skills": [,]}"#,
                r#"{"skills": ["a",,]}"#,
                r#"{"skills": ["unterminated]}"#,
            ] {
                assert_eq!(plan_edit(text, &new, None, Mode::New), None, "{text}");
            }
        }

        #[test]
        fn first_install_creates_a_private_file_and_only_reload_follows_a_change() {
            let home = TestHome::new();
            let desired = pair("dtw", "new");
            let calls = RefCell::new(Vec::new());
            let setup = |mode| {
                register_default(&home.0, &home.0, &desired, None, mode, |args| {
                    calls.borrow_mut().push(args.join(" "));
                    if args == ["reload"] {
                        assert_eq!(fs::read_to_string(home.file()).unwrap(), desired.document());
                    }
                    Some(home.run(args))
                })
            };
            assert_eq!(setup(Mode::Current)["state"], "host_action_required");
            assert!(!home.file().exists());
            assert!(calls.borrow().is_empty());
            assert_eq!(setup(Mode::New)["state"], "verify");
            assert_eq!(fs::metadata(home.file()).unwrap().mode() & 0o777, 0o600);
            assert_eq!(setup(Mode::Resume)["state"], "verify");
            assert_eq!(setup(Mode::Current)["state"], "verify");
            assert_eq!(
                *calls.borrow(),
                [
                    "--version",
                    "debug paths",
                    "reload",
                    "--version",
                    "debug paths",
                    "--version",
                    "debug paths"
                ]
            );
            assert_eq!(
                fs::read_dir(home.file().parent().unwrap()).unwrap().count(),
                1
            );
        }

        #[test]
        fn failed_reload_leaves_the_edit_and_private_backup_for_manual_reload() {
            let home = TestHome::new();
            let old = pair("dtw", "old");
            let new = pair("dtw", "new");
            let path = home.file();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let original = format!("// preserve credentials and comments\n{}", old.document());
            fs::write(&path, &original).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
            let result = register_default(&home.0, &home.0, &new, Some(&old), Mode::New, |args| {
                (args != ["reload"]).then(|| home.run(args))
            });
            assert_eq!(result["state"], "host_action_required");
            assert_eq!(result["next"], RELOAD);
            let backup = Path::new(result["backup"].as_str().unwrap());
            assert_eq!(fs::read_to_string(backup).unwrap(), original);
            assert_eq!(fs::metadata(backup).unwrap().mode() & 0o777, 0o600);
            assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
            let updated = fs::read_to_string(&path).unwrap();
            assert_eq!(
                updated,
                original
                    .replace(&old.executable, &new.executable)
                    .replace(&old.skill, &new.skill)
            );
        }

        #[test]
        fn refuses_workspace_overrides_symlinks_and_unconfirmed_host_scope() {
            let home = TestHome::new();
            let desired = pair("dtw", "new");
            let cwd = home.0.join("project");
            fs::create_dir(&cwd).unwrap();
            let overrides = cwd.join(".opencode");
            symlink("nonexistent", &overrides).unwrap();
            let result = register_default(&home.0, &cwd, &desired, None, Mode::New, |_| {
                panic!("ambiguous workspace scope")
            });
            assert_eq!(result["next"], REVIEW_SCOPE);
            fs::remove_file(overrides).unwrap();
            let result = register_default(&home.0, &cwd, &desired, None, Mode::New, |args| {
                assert_eq!(args, ["--version"]);
                Some(b"opencode v2.3.1\n".to_vec())
            });
            assert_eq!(result["state"], "host_action_required");
            let result =
                register_default(
                    &home.0,
                    &cwd,
                    &desired,
                    None,
                    Mode::New,
                    |args| match args {
                        ["--version"] => Some(home.run(args)),
                        ["debug", "paths"] => {
                            Some(b"home /somewhere\nconfig /somewhere/else".to_vec())
                        }
                        _ => panic!("unexpected host command"),
                    },
                );
            assert_eq!(result["next"], REVIEW_SCOPE);
            let path = home.file();
            assert!(!path.exists());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let other = home.0.join("unrelated-config");
            fs::write(&other, "private").unwrap();
            symlink(&other, &path).unwrap();
            assert_eq!(
                register_default(&home.0, &cwd, &desired, None, Mode::New, |args| {
                    assert_ne!(args, ["reload"]);
                    Some(home.run(args))
                })["next"],
                REVIEW_SCOPE
            );
            assert_eq!(fs::read_to_string(other).unwrap(), "private");
        }
    }
}
