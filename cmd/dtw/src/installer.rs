//! Host adapters around the complete, relocatable DTW package.
use super::{copy_tree, data_root, executable_name, package_root};
use serde_json::{json, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
use toml_edit::{value, DocumentMut, Item, Table};

const OWNER: &str = ".dtw-owner.json";
const ID: &str = "desktop-world";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Agent {
    Codex,
    Claude,
    Antigravity,
    Gemini,
    Cursor,
}
impl Agent {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "agy" | "antigravity" => Ok(Self::Antigravity),
            "gemini" => Ok(Self::Gemini),
            "cursor" => Ok(Self::Cursor),
            _ => Err(usage()),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Antigravity => "antigravity",
            Self::Gemini => "gemini",
            Self::Cursor => "cursor",
        }
    }
}

pub(super) fn usage() -> String {
    "usage:\n  dtw install agy|claude|codex|gemini|cursor [--scope user | --project EXISTING_DIR]\n  dtw uninstall agy|claude|codex|gemini|cursor [--scope user | --project EXISTING_DIR]\n  dtw install plugin --to NEW_DIRECTORY\n\n'antigravity' is an alias for 'agy'; gemini and claude support user scope only".into()
}
fn home() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let path = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"));
    #[cfg(not(windows))]
    let path = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"));
    let path = path.ok_or("HOME or USERPROFILE is required")?;
    let path = PathBuf::from(path);
    if !path.is_dir() {
        return Err(format!("home does not exist: {}", path.display()));
    }
    Ok(path.canonicalize().map_err(|e| e.to_string())?)
}
fn absolute_existing(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path)
        .canonicalize()
        .map_err(|e| format!("project: {e}"))?;
    if !p.is_dir() {
        return Err("project must be an existing directory".into());
    }
    Ok(p)
}
fn owned(path: &Path, agent: Agent) -> Result<bool, String> {
    if fs::symlink_metadata(path).is_err() {
        return Ok(false);
    }
    if !path.is_dir() || path.is_symlink() {
        return Err(format!("not a managed DTW directory: {}", path.display()));
    }
    let marker: Value = serde_json::from_slice(&fs::read(path.join(OWNER)).map_err(|_| {
        format!(
            "existing directory is not owned by this installer: {}",
            path.display()
        )
    })?)
    .map_err(|e| e.to_string())?;
    if marker["id"] != ID || marker["agent"] != agent.name() {
        return Err(format!(
            "existing directory is not owned by this installer: {}",
            path.display()
        ));
    }
    Ok(true)
}
fn stage_copy(
    source: &Path,
    destination: &Path,
    agent: Agent,
    package: bool,
) -> Result<(), String> {
    let parent = destination.parent().ok_or("invalid install path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let previous = owned(destination, agent)?;
    let stage = parent.join(format!(".dtw-stage-{}", uuid::Uuid::new_v4()));
    let backup = parent.join(format!(".dtw-backup-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        if package {
            super::canonical_copy(source, &stage)?;
        } else {
            copy_tree(source, &stage)?;
        }
        fs::write(
            stage.join(OWNER),
            format!("{}\n", json!({"id":ID,"agent":agent.name()})),
        )
        .map_err(|e| e.to_string())?;
        if package {
            // Adapted files and the ownership marker are outside the release checksum set.
            let _ = fs::remove_file(stage.join("SHA256SUMS"));
            match agent {
                Agent::Antigravity => {
                    fs::write(stage.join("plugin.json"), format!("{}\n", json!({"$schema":"https://antigravity.google/schemas/v1/plugin.json","name":ID,"description":"Desktop World native CUA integration"}))).map_err(|e| e.to_string())?;
                    write_json(
                        &stage.join("mcp_config.json"),
                        &json!({"mcpServers":{"dtw":{"command":destination.join("bin").join(executable_name("dtw"))}}}),
                    )?;
                }
                Agent::Gemini => {
                    let manifest = json!({"name":ID,"version":env!("CARGO_PKG_VERSION"),"description":"Desktop World native CUA integration","mcpServers":{ID:{"command":format!("${{extensionPath}}${{/}}bin${{/}}{}",executable_name("dtw"))}}});
                    write_json(&stage.join("gemini-extension.json"), &manifest)?;
                }
                Agent::Claude => {
                    write_json(
                        &stage.join(".mcp.json"),
                        &json!({"mcpServers":{ID:{"command":format!("${{CLAUDE_PLUGIN_ROOT}}/bin/{}",executable_name("dtw"))}}}),
                    )?;
                }
                _ => {}
            }
        }
        if previous {
            fs::rename(destination, &backup).map_err(|e| e.to_string())?;
        }
        if let Err(e) = fs::rename(&stage, destination) {
            if previous {
                let _ = fs::rename(&backup, destination);
            }
            return Err(e.to_string());
        }
        if previous {
            fs::remove_dir_all(&backup).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    fs::write(path, [bytes, b"\n".to_vec()].concat()).map_err(|e| e.to_string())
}
fn read_json(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("{}: {e}", path.display()))
}
fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("invalid config path")?).map_err(|e| e.to_string())?;
    if path.is_symlink() {
        return Err(format!("refusing symlink: {}", path.display()));
    }
    let temp = path.with_extension(format!("dtw-{}", uuid::Uuid::new_v4()));
    fs::write(&temp, contents).map_err(|e| e.to_string())?;
    // On Windows rename does not replace an existing destination; preserve its contents
    // through a same-directory backup and restore on failure.
    let backup = path.with_extension(format!("dtw-backup-{}", uuid::Uuid::new_v4()));
    let existed = path.exists();
    if existed {
        fs::rename(path, &backup).map_err(|e| e.to_string())?;
    }
    if let Err(e) = fs::rename(&temp, path) {
        if existed {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temp);
        return Err(e.to_string());
    }
    if existed {
        fs::remove_file(backup).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn json_mcp(path: &Path, command: &Path, remove: bool) -> Result<(), String> {
    let mut doc = read_json(path)?;
    let root = doc
        .as_object_mut()
        .ok_or("MCP config root must be an object")?;
    let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
    let map = servers
        .as_object_mut()
        .ok_or("mcpServers must be an object")?;
    if let Some(existing) = map.get(ID) {
        if existing.get("command").and_then(Value::as_str) != command.to_str() {
            return Err(format!(
                "refusing to replace unrelated MCP server {ID} in {}",
                path.display()
            ));
        }
    }
    if remove {
        map.remove(ID);
    } else {
        map.insert(ID.into(), json!({"command":command}));
    }
    if remove && !path.exists() {
        return Ok(());
    }
    let mut bytes = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}
fn codex_mcp(path: &Path, command: &Path, remove: bool) -> Result<(), String> {
    let input = if path.exists() {
        fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let mut doc = input
        .parse::<DocumentMut>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !doc.as_table().contains_key("mcp_servers") {
        doc["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = doc["mcp_servers"]
        .as_table_mut()
        .ok_or("mcp_servers must be a table")?;
    if let Some(existing) = servers.get(ID) {
        if existing.get("command").and_then(Item::as_str) != command.to_str() {
            return Err(format!(
                "refusing to replace unrelated MCP server {ID} in {}",
                path.display()
            ));
        }
    }
    if remove {
        servers.remove(ID);
    } else {
        let mut table = Table::new();
        table["command"] = value(command.to_string_lossy().as_ref());
        servers[ID] = Item::Table(table);
    }
    if remove && !path.exists() {
        return Ok(());
    }
    write_atomic(path, doc.to_string().as_bytes())
}
fn cli(name: &str, args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new(name);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("{name} CLI is required ({e}); install {name} and retry"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{name} command failed ({}): {}",
            output.status,
            err.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
fn cli_list(name: &str, args: &[&str], cwd: Option<&Path>) -> Result<Value, String> {
    serde_json::from_str(&cli(name, args, cwd)?).map_err(|e| format!("{name} list output: {e}"))
}
fn array_has_name(v: &Value, needle: &str) -> bool {
    v.as_array()
        .map(|a| {
            a.iter()
                .any(|x| x["name"] == needle || x["id"] == needle || x["pluginId"] == needle)
        })
        .unwrap_or(false)
}
fn agy_imported(v: &Value) -> bool {
    array_has_name(&v["imports"], ID)
}
fn agy_list() -> Result<Value, String> {
    let output = cli("agy", &["plugin", "list"], None)?;
    if output.trim() == "No imported plugins." {
        return Ok(json!({"imports":[]}));
    }
    serde_json::from_str(&output).map_err(|e| format!("agy plugin list output: {e}"))
}

fn agy_install(package: &Path, target: &Path) -> Result<(), String> {
    let listing = agy_list()?;
    let existing = owned(target, Agent::Antigravity)?;
    if agy_imported(&listing) && !existing {
        return Err("Antigravity already has a desktop-world plugin not owned by DTW".into());
    }
    let stage = data_root()?.join(format!(".agy-stage-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        stage_copy(package, &stage, Agent::Antigravity, true)?;
        // The CLI copies this package. Its MCP command must refer to the final
        // installed path, not the temporary source directory.
        write_json(
            &stage.join("mcp_config.json"),
            &json!({"mcpServers":{"dtw":{"command":target.join("bin").join(executable_name("dtw"))}}}),
        )?;
        cli(
            "agy",
            &["plugin", "install", stage.to_str().ok_or("non-UTF8 path")?],
            None,
        )?;
        if !owned(target, Agent::Antigravity)? || !agy_imported(&agy_list()?) {
            return Err("Antigravity did not register the DTW plugin".into());
        }
        let config = read_json(&target.join("mcp_config.json"))?;
        if config["mcpServers"]["dtw"]["command"]
            != target
                .join("bin")
                .join(executable_name("dtw"))
                .to_string_lossy()
                .as_ref()
        {
            return Err("Antigravity installed an unexpected MCP command".into());
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(stage);
    result
}
fn claude_marketplace(root: &Path, plugin: &Path) -> Result<(), String> {
    fs::create_dir_all(root.join(".claude-plugin")).map_err(|e| e.to_string())?;
    let manifest = json!({"name":"dtw-local","owner":{"name":"Caelis Labs"},"plugins":[{"name":ID,"source":"./plugins/desktop-world"}]});
    // A pre-existing marketplace root is accepted only when its sole plugin is ours.
    let path = root.join(".claude-plugin/marketplace.json");
    if path.exists() && read_json(&path)? != manifest {
        return Err("marketplace root is owned by another plugin".into());
    }
    if !plugin.is_dir() {
        return Err("Claude plugin package missing".into());
    }
    write_json(&path, &manifest)
}
fn remove_owned(path: &Path, agent: Agent) -> Result<(), String> {
    if owned(path, agent)? {
        fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn run(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err(usage());
    }
    let install = match args[0].as_str() {
        "install" => true,
        "uninstall" => false,
        _ => return Err(usage()),
    };
    let agent = Agent::parse(&args[1])?;
    let (scope, project) = match &args[2..] {
        [] => ("user", None),
        [flag, value] if flag == "--scope" && value == "user" => ("user", None),
        [flag, value] if flag == "--project" => ("project", Some(absolute_existing(value)?)),
        _ => return Err(usage()),
    };
    if agent == Agent::Claude && project.is_some() {
        return Err("Claude supports user scope only in v0.1.0; project marketplace identity is not supported".into());
    }
    if agent == Agent::Gemini && project.is_some() {
        return Err("Gemini extensions install at user scope; use --scope user".into());
    }
    let h = home()?;
    let base = project.as_deref().unwrap_or(&h);
    let package = package_root()?;
    let root = if agent == Agent::Claude {
        if scope == "user" {
            data_root()?.join("claude-marketplace")
        } else {
            base.join(".dtw/claude-marketplace")
        }
    } else if scope == "user" {
        data_root()?.join(agent.name())
    } else {
        base.join(".agents/plugins").join(agent.name())
    };
    let target = if agent == Agent::Antigravity {
        if scope == "user" {
            h.join(".gemini/config/plugins/desktop-world")
        } else {
            base.join(".agents/plugins/desktop-world")
        }
    } else if agent == Agent::Claude {
        root.join("plugins/desktop-world")
    } else {
        root.join("desktop-world")
    };
    let skill = match agent {
        Agent::Codex => Some(if scope == "user" {
            h.join(".agents/skills/desktop-world")
        } else {
            base.join(".agents/skills/desktop-world")
        }),
        Agent::Cursor => Some(if scope == "user" {
            h.join(".cursor/skills/desktop-world")
        } else {
            base.join(".cursor/skills/desktop-world")
        }),
        _ => None,
    };
    let config = match agent {
        Agent::Codex => Some(if scope == "user" {
            h.join(".codex/config.toml")
        } else {
            base.join(".codex/config.toml")
        }),
        Agent::Cursor => Some(if scope == "user" {
            h.join(".cursor/mcp.json")
        } else {
            base.join(".cursor/mcp.json")
        }),
        _ => None,
    };
    let command = target.join("bin").join(executable_name("dtw"));
    // Preflight every file we may touch before replacing the complete runtime.
    if let Some(ref s) = skill {
        owned(s, agent)?;
    }
    if let Some(ref c) = config {
        if agent == Agent::Codex {
            codex_preflight(c, &command)?;
        } else {
            json_preflight(c, &command)?;
        }
    }
    if agent == Agent::Claude {
        let markets = cli_list("claude", &["plugin", "marketplace", "list", "--json"], None)?;
        if let Some(market) = markets
            .as_array()
            .and_then(|a| a.iter().find(|m| m["name"] == "dtw-local"))
        {
            let location = market["installLocation"].as_str().map(PathBuf::from);
            if market["source"] != "directory"
                || location.as_deref() != Some(root.as_path())
                || !owned(&target, agent)?
            {
                return Err("dtw-local marketplace is not the owned DTW source; refusing to replace or reuse it".into());
            }
        }
    }
    if install {
        if agent == Agent::Antigravity && scope == "user" {
            agy_install(&package, &target)?;
            println!(
                "{}",
                json!({"agent":"agy","scope":scope,"plugin":target,"mcp_command":command,"skill":target.join("skills/desktop-world")})
            );
            return Ok(());
        }
        if agent == Agent::Claude {
            cli("claude", &["plugin", "--help"], project.as_deref())?;
        }
        if agent == Agent::Gemini {
            cli("gemini", &["extensions", "--help"], None)?;
        }
        stage_copy(&package, &target, agent, true)?;
        if let Some(ref s) = skill {
            stage_copy(&package.join("skills/desktop-world"), s, agent, false)?;
        }
        if let Some(ref c) = config {
            if agent == Agent::Codex {
                codex_mcp(c, &command, false)?;
            } else {
                json_mcp(c, &command, false)?;
            }
        }
        if agent == Agent::Claude {
            claude_marketplace(&root, &target)?;
            let markets = cli_list(
                "claude",
                &["plugin", "marketplace", "list", "--json"],
                project.as_deref(),
            )?;
            if !array_has_name(&markets, "dtw-local") {
                cli(
                    "claude",
                    &[
                        "plugin",
                        "marketplace",
                        "add",
                        root.to_str().ok_or("non-UTF8 path")?,
                        "--scope",
                        scope,
                    ],
                    project.as_deref(),
                )?;
            }
            let plugins = cli_list("claude", &["plugin", "list", "--json"], project.as_deref())?;
            if !plugins.as_array().is_some_and(|items| {
                items
                    .iter()
                    .any(|p| p["id"] == "desktop-world@dtw-local" && p["scope"] == "user")
            }) {
                cli(
                    "claude",
                    &[
                        "plugin",
                        "install",
                        "desktop-world@dtw-local",
                        "--scope",
                        scope,
                    ],
                    project.as_deref(),
                )?;
            } else {
                cli(
                    "claude",
                    &[
                        "plugin",
                        "update",
                        "desktop-world@dtw-local",
                        "--scope",
                        scope,
                    ],
                    project.as_deref(),
                )?;
            }
        }
        if agent == Agent::Gemini {
            let extension = h.join(".gemini/extensions/desktop-world");
            if extension.exists() {
                if !extension.join(OWNER).exists() {
                    return Err("Gemini extension with this name is not owned by DTW".into());
                }
                cli("gemini", &["extensions", "uninstall", ID], None)?;
            }
            cli(
                "gemini",
                &[
                    "extensions",
                    "install",
                    target.to_str().ok_or("non-UTF8 path")?,
                    "--consent",
                ],
                None,
            )?;
        }
        println!(
            "{}",
            json!({"agent":agent.name(),"scope":scope,"plugin":target,"mcp_command":command,"skill":skill})
        );
    } else {
        if !owned(&target, agent)? {
            return Err(format!("{} is not installed by DTW", agent.name()));
        }
        if agent == Agent::Antigravity && scope == "user" {
            if !agy_imported(&agy_list()?) {
                return Err("Antigravity does not list the DTW plugin; refusing to remove files behind its back".into());
            }
            cli("agy", &["plugin", "uninstall", ID], None)?;
            if target.exists() || agy_imported(&agy_list()?) {
                return Err("Antigravity did not fully remove the DTW plugin".into());
            }
            println!("{}", json!({"agent":"agy","scope":scope,"removed":ID}));
            return Ok(());
        }
        if agent == Agent::Claude {
            let plugins = cli_list("claude", &["plugin", "list", "--json"], project.as_deref())?;
            if array_has_name(&plugins, "desktop-world@dtw-local") {
                cli(
                    "claude",
                    &[
                        "plugin",
                        "uninstall",
                        "desktop-world@dtw-local",
                        "--scope",
                        scope,
                    ],
                    project.as_deref(),
                )?;
            }
            let markets = cli_list(
                "claude",
                &["plugin", "marketplace", "list", "--json"],
                project.as_deref(),
            )?;
            if array_has_name(&markets, "dtw-local") {
                cli(
                    "claude",
                    &[
                        "plugin",
                        "marketplace",
                        "remove",
                        "dtw-local",
                        "--scope",
                        scope,
                    ],
                    project.as_deref(),
                )?;
            }
        }
        if agent == Agent::Gemini && h.join(".gemini/extensions/desktop-world").exists() {
            cli("gemini", &["extensions", "uninstall", ID], None)?;
        }
        if let Some(ref c) = config {
            if agent == Agent::Codex {
                codex_mcp(c, &command, true)?;
            } else {
                json_mcp(c, &command, true)?;
            }
        }
        if let Some(ref s) = skill {
            remove_owned(s, agent)?;
        }
        remove_owned(&target, agent)?;
        if agent == Agent::Claude {
            let _ = fs::remove_file(root.join(".claude-plugin/marketplace.json"));
            let _ = fs::remove_dir(root.join(".claude-plugin"));
            let _ = fs::remove_dir(root.join("plugins"));
            let _ = fs::remove_dir(root);
        }
        println!(
            "{}",
            json!({"agent":agent.name(),"scope":scope,"removed":ID})
        );
    }
    Ok(())
}
fn codex_preflight(path: &Path, command: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let doc = fs::read_to_string(path)
        .map_err(|e| e.to_string())?
        .parse::<DocumentMut>()
        .map_err(|e| e.to_string())?;
    if let Some(entry) = doc.get("mcp_servers").and_then(|x| x.get(ID)) {
        if entry.get("command").and_then(Item::as_str) != command.to_str() {
            return Err(format!(
                "refusing to replace unrelated MCP server {ID} in {}",
                path.display()
            ));
        }
    }
    Ok(())
}
fn json_preflight(path: &Path, command: &Path) -> Result<(), String> {
    let doc = read_json(path)?;
    if let Some(entry) = doc.get("mcpServers").and_then(|x| x.get(ID)) {
        if entry.get("command").and_then(Value::as_str) != command.to_str() {
            return Err(format!(
                "refusing to replace unrelated MCP server {ID} in {}",
                path.display()
            ));
        }
    }
    Ok(())
}
