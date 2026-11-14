//! Complete package verification. Never follow links or execute imported files.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

pub(super) type Result<T> = std::result::Result<T, super::PackageError>;
pub(super) fn io_error(error: io::Error) -> super::PackageError {
    super::PackageError {
        os_error: error.raw_os_error(),
        ..error.to_string().into()
    }
}
pub(super) fn text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| "Path is not valid Unicode".into())
}
pub(super) fn json(path: &Path) -> Result<Value> {
    regular(path, false)?;
    let mut data = Vec::new();
    fs::File::open(path)
        .map_err(io_error)?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(io_error)?;
    if data.len() > 2 * 1024 * 1024 {
        return Err("Metadata exceeds 2 MiB".into());
    }
    serde_json::from_slice(&data).map_err(|e| e.to_string().into())
}
pub(super) fn digest(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(io_error)?;
    if file.metadata().map_err(io_error)?.len() > 128 * 1024 * 1024 {
        return Err("Package file exceeds 128 MiB".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(io_error)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(super) fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}
pub(super) fn regular(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if linked(&metadata)
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        }
    {
        return Err(format!(
            "Expected an owned regular {}: {}",
            if directory { "directory" } else { "file" },
            path.display()
        )
        .into());
    }
    Ok(())
}
pub(super) fn open_lock_file(path: &Path) -> Result<fs::File> {
    match fs::symlink_metadata(path) {
        Ok(_) => regular(path, false)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    let file = options.open(path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if linked(&metadata) || !metadata.is_file() {
        return Err("Lock must be a regular file, not a link/reparse point".into());
    }
    Ok(file)
}
pub(super) fn inventory(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    regular(root, true)?;
    let mut pending = vec![root.to_owned()];
    let mut files = BTreeMap::new();
    let mut entries = 0usize;
    while let Some(directory) = pending.pop() {
        for row in fs::read_dir(directory).map_err(io_error)? {
            let path = row.map_err(io_error)?.path();
            entries += 1;
            if entries > 8192 {
                return Err("Package inventory exceeds 8192 entries".into());
            }
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if linked(&metadata) {
                return Err(
                    format!("Package contains a link/reparse point: {}", path.display()).into(),
                );
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                let name =
                    text(path.strip_prefix(root).map_err(|e| e.to_string())?)?.replace('\\', "/");
                files.insert(name, path);
            } else {
                return Err("Package contains a special file".into());
            }
        }
    }
    Ok(files)
}
pub(super) fn relative(name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name.contains(['\\', ':', '\0'])
        || name.starts_with('/')
        || name.chars().any(char::is_control)
    {
        return Err("Unsafe package member".into());
    }
    let path = PathBuf::from(name);
    if !path
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        || name.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(['.', ' '])
                || reserved_name(part)
        })
    {
        return Err("Unsafe package member components".into());
    }
    Ok(path)
}
pub(super) fn reserved_name(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}
#[derive(Clone, Debug)]
pub(super) struct Verified {
    pub id: String,
    pub build: Value,
    pub files: BTreeMap<String, PathBuf>,
}
pub(super) fn verify(root: &Path) -> Result<Verified> {
    let files = inventory(root)?;
    let checksum = files.get("SHA256SUMS").ok_or("Missing SHA256SUMS")?;
    let mut sums = String::new();
    fs::File::open(checksum)
        .map_err(io_error)?
        .take(2 * 1024 * 1024 + 1)
        .read_to_string(&mut sums)
        .map_err(io_error)?;
    if sums.len() > 2 * 1024 * 1024 {
        return Err("Checksum inventory too large".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut folded = std::collections::HashSet::new();
    for line in sums.lines() {
        let (expected, name) = line.split_once("  ").ok_or("Malformed package checksum")?;
        relative(name)?;
        if expected.len() != 64
            || !expected
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            || name == "SHA256SUMS"
            || !seen.insert(name)
            || !folded.insert(name.to_lowercase())
        {
            return Err("Duplicate or invalid package checksum".into());
        }
        let path = files
            .get(name)
            .ok_or_else(|| format!("Missing package member: {name}"))?;
        if digest(path)? != expected {
            return Err(format!("Package checksum mismatch: {name}").into());
        }
    }
    if seen.len() + 1 != files.len() {
        return Err("Unlisted package files".into());
    }
    let build = json(&root.join("BUILD.json"))?;
    let (skill_dir, other_skill_dir) = match build["package_store_schema"].as_u64() {
        Some(1) => ("skills/desktop-world", "skills/dtw"),
        Some(2) => ("skills/dtw", "skills/desktop-world"),
        _ => return Err("Unsupported managed package layout or runtime leases".into()),
    };
    if root.join(other_skill_dir).exists() {
        return Err("Package Skill layout mismatch".into());
    }
    let version = build["version"].as_str().ok_or("Missing package version")?;
    if version.split('.').count() != 3
        || !version
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Invalid package version".into());
    }
    let target = build["target"].as_str().ok_or("Missing package target")?;
    let binary = match target {
        "aarch64-apple-darwin" => "bin/dtw",
        "x86_64-pc-windows-msvc" => "bin/dtw.exe",
        _ => return Err("Unsupported package target".into()),
    };
    if build["binary"]["path"] != binary || build["binary"]["sha256"] != digest(&root.join(binary))?
    {
        return Err("BUILD executable identity mismatch".into());
    }
    for name in ["SKILL.md", "API.md", "PATTERNS.md"] {
        regular(&root.join(skill_dir).join(name), false)?;
    }
    if build["skill_sha256"] != digest(&root.join(skill_dir).join("SKILL.md"))? {
        return Err("BUILD Skill identity mismatch".into());
    }
    for name in [
        "plugin.json",
        ".claude-plugin/plugin.json",
        ".antigravity-plugin/plugin.json",
        "package.json",
    ] {
        if json(&root.join(name))?["version"] != version {
            return Err("Plugin/package version mismatch".into());
        }
    }
    Ok(Verified {
        id: digest(checksum)?,
        build,
        files,
    })
}
pub(super) fn copy(source: &Path, destination: &Path, verified: &Verified) -> Result<()> {
    fs::create_dir(destination).map_err(io_error)?;
    for (name, path) in &verified.files {
        let target = destination.join(relative(name)?);
        fs::create_dir_all(target.parent().unwrap()).map_err(io_error)?;
        regular(path, false)?;
        fs::copy(path, &target).map_err(io_error)?;
    }
    if verify(destination)?.id != verified.id {
        return Err("Package changed while importing".into());
    }
    let _ = source;
    Ok(())
}
pub(super) fn file_url(path: &Path) -> Result<String> {
    url::Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| "Cannot encode absolute package file URL".into())
}
pub(super) fn write(path: &Path, value: &Value) -> Result<()> {
    use io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error)?;
    let data = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    file.write_all(&data)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(io_error)
}

// Preserve ownership metadata until payload removal succeeds. In particular a
// Windows sharing violation must not turn recoverable trash into unknown data.
pub(super) fn remove_owned_tree(path: &Path) -> Result<()> {
    regular(path, true)?;
    for entry in fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name();
        if !["payload", "PACKAGE.json", "STAGE.json"]
            .iter()
            .any(|s| name == *s)
        {
            return Err("Unknown data in owned package wrapper; preserving it".into());
        }
        regular(&entry.path(), name == "payload")?;
    }
    let payload = path.join("payload");
    if payload.exists() {
        regular(&payload, true)?;
        fs::remove_dir_all(payload).map_err(io_error)?;
    }
    for name in ["PACKAGE.json", "STAGE.json"] {
        let file = path.join(name);
        if file.exists() {
            regular(&file, false)?;
            fs::remove_file(file).map_err(io_error)?;
        }
    }
    fs::remove_dir(path).map_err(io_error)
}
