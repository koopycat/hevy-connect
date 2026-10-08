//! Ambient-context hooks: a SessionStart hook for Claude Code and Codex, and a
//! plugin for OpenCode, each running `hevy-axi` so every agent session starts
//! knowing whether Hevy is configured. Only entries carrying this tool's
//! marker are ever added, changed, or removed.

use std::fs::{self, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use toml_edit::{DocumentMut, value};

use crate::config::Environment;
use crate::error::{Error, Result};

const MARKER: &str = "hevy-axi";
const TIMEOUT_SECONDS: u64 = 10;
const PLUGIN_NAME: &str = "axi-hevy-axi.js";
const PLUGIN_TEMPLATE: &str = include_str!("opencode_plugin.js");
const PLUGIN_MANAGED_MARKER: &str = "axi-sdk-js managed opencode plugin: hevy-axi";

struct Targets {
    claude_settings: PathBuf,
    codex_hooks: PathBuf,
    codex_config: PathBuf,
    opencode_plugin: PathBuf,
}

impl Targets {
    fn of(home: &Path) -> Self {
        Self {
            claude_settings: home.join(".claude/settings.json"),
            codex_hooks: home.join(".codex/hooks.json"),
            codex_config: home.join(".codex/config.toml"),
            opencode_plugin: home.join(".config/opencode/plugins").join(PLUGIN_NAME),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    pub claude: bool,
    pub codex: bool,
    pub opencode: bool,
}

fn failed(path: &Path, what: &str) -> Error {
    Error::config(
        format!("The hook configuration could not be updated: {what}."),
        path,
    )
}

fn is_managed(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.contains(MARKER))
}

fn managed_hook_groups(settings: &Value) -> impl Iterator<Item = &Value> {
    settings["hooks"]["SessionStart"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
}

/// Write `contents` so a crash never leaves a partial file. A replaced file
/// keeps its permissions; a new one gets the usual `0666 & ~umask`.
fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    let directory = path.parent().expect("hook targets have a parent");
    fs::create_dir_all(directory)?;
    let mut file = tempfile::Builder::new()
        .permissions(Permissions::from_mode(0o666))
        .tempfile_in(directory)?;
    file.write_all(contents.as_bytes())?;
    if let Ok(existing) = fs::metadata(path) {
        fs::set_permissions(file.path(), existing.permissions())?;
    }
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(failed(path, "it could not be read")),
    }
}

/// The settings with our SessionStart hook present and correct, or `None` if
/// they already are.
fn with_hook(settings: &Value, command: &str) -> std::result::Result<Option<Value>, &'static str> {
    let mut updated = settings.clone();
    let root = updated.as_object_mut().ok_or("it is not a JSON object")?;
    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    let groups = hooks
        .as_object_mut()
        .ok_or("its \"hooks\" entry is not an object")?
        .entry("SessionStart")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("its \"SessionStart\" entry is not an array")?;

    let wanted = json!({ "type": "command", "command": command, "timeout": TIMEOUT_SECONDS });
    let existing = groups
        .iter_mut()
        .filter_map(|group| group.get_mut("hooks").and_then(Value::as_array_mut))
        .flatten()
        .find(|hook| is_managed(hook));
    match existing {
        Some(hook) if *hook == wanted => {}
        Some(hook) => {
            hook["command"] = command.into();
            hook["type"] = "command".into();
            hook["timeout"] = TIMEOUT_SECONDS.into();
        }
        None => groups.push(json!({ "matcher": "", "hooks": [wanted] })),
    }
    Ok((updated != *settings).then_some(updated))
}

/// The settings without any of our hooks, or `None` if there were none.
fn without_hook(settings: &Value) -> Option<Value> {
    let mut updated = settings.clone();
    let hooks = updated.get_mut("hooks")?.as_object_mut()?;
    let groups = hooks.get_mut("SessionStart")?.as_array_mut()?;

    let mut remaining = Vec::new();
    for mut group in std::mem::take(groups) {
        let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            remaining.push(group);
            continue;
        };
        let before = entries.len();
        entries.retain(|hook| !is_managed(hook));
        if entries.len() == before || !entries.is_empty() {
            remaining.push(group);
        }
    }
    if remaining.is_empty() {
        hooks.remove("SessionStart");
    } else {
        *groups = remaining;
    }
    if hooks.is_empty() {
        updated.as_object_mut()?.remove("hooks");
    }
    (updated != *settings).then_some(updated)
}

fn parse_json(path: &Path, text: &str) -> Result<Value> {
    serde_json::from_str(text).map_err(|_| failed(path, "it is not valid JSON"))
}

fn install_json_hook(path: &Path, command: &str) -> Result<()> {
    let current = match read_optional(path)? {
        Some(text) => parse_json(path, &text)?,
        None => json!({}),
    };
    if let Some(updated) = with_hook(&current, command).map_err(|why| failed(path, why))? {
        let text = serde_json::to_string_pretty(&updated).expect("a JSON value always serializes");
        write_atomically(path, &(text + "\n"))
            .map_err(|_| failed(path, "it could not be written"))?;
    }
    Ok(())
}

fn remove_json_hook(path: &Path) -> Result<()> {
    let Some(text) = read_optional(path)? else {
        return Ok(());
    };
    if let Some(updated) = without_hook(&parse_json(path, &text)?) {
        let text = serde_json::to_string_pretty(&updated).expect("a JSON value always serializes");
        write_atomically(path, &(text + "\n"))
            .map_err(|_| failed(path, "it could not be written"))?;
    }
    Ok(())
}

/// Codex ignores hooks unless `[features] hooks = true` is set.
fn enable_codex_hooks(path: &Path) -> Result<()> {
    let text = read_optional(path)?.unwrap_or_default();
    let mut document: DocumentMut = text
        .parse()
        .map_err(|_| failed(path, "it is not valid TOML"))?;
    let features = document
        .entry("features")
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .ok_or_else(|| failed(path, "its \"features\" entry is not a table"))?;
    if features.get("hooks").and_then(|hooks| hooks.as_bool()) == Some(true) {
        return Ok(());
    }
    match features
        .get_mut("hooks")
        .and_then(|item| item.as_value_mut())
    {
        // Flip a `hooks = false` in place, keeping its trailing comment.
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = true.into();
            *existing.decor_mut() = decor;
        }
        None => {
            features.insert("hooks", value(true));
        }
    }
    write_atomically(path, &document.to_string())
        .map_err(|_| failed(path, "it could not be written"))
}

fn plugin_source(command: &str) -> String {
    let command = serde_json::to_string(command).expect("a string always serializes");
    PLUGIN_TEMPLATE.replace("{{COMMAND}}", &command)
}

fn is_managed_plugin(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|text| text.contains(PLUGIN_MANAGED_MARKER))
}

fn install_plugin(path: &Path, command: &str) -> Result<()> {
    if path.exists() && !is_managed_plugin(path) {
        return Err(failed(
            path,
            "an unmanaged OpenCode plugin is already there",
        ));
    }
    let source = plugin_source(command);
    if fs::read_to_string(path).is_ok_and(|current| current == source) {
        return Ok(());
    }
    write_atomically(path, &source).map_err(|_| failed(path, "it could not be written"))
}

/// The command the hooks run: the bare name when that resolves to this very
/// executable on `PATH` (so the hook survives reinstalls), else its full path.
fn hook_command(exe: &Path, path_variable: Option<&str>) -> String {
    let resolved = fs::canonicalize(exe).ok();
    let on_path = path_variable.is_some_and(|paths| {
        std::env::split_paths(paths)
            .filter(|dir| !dir.as_os_str().is_empty())
            .any(|dir| {
                let candidate = fs::canonicalize(dir.join(MARKER)).ok();
                candidate.is_some() && candidate == resolved
            })
    });
    if on_path {
        MARKER.to_owned()
    } else {
        exe.display().to_string()
    }
}

/// Install the hooks for `exe` in every supported agent.
pub fn install(env: &Environment, exe: &Path) -> Result<()> {
    let targets = Targets::of(&env.home);
    let command = hook_command(exe, env.path_variable());
    install_plugin(&targets.opencode_plugin, &command)?;
    install_json_hook(&targets.claude_settings, &command)?;
    install_json_hook(&targets.codex_hooks, &command)?;
    enable_codex_hooks(&targets.codex_config)
}

/// Remove only our hooks. Codex's shared `[features] hooks` switch stays on,
/// since other tools may rely on it.
pub fn uninstall(env: &Environment) -> Result<()> {
    let targets = Targets::of(&env.home);
    remove_json_hook(&targets.claude_settings)?;
    remove_json_hook(&targets.codex_hooks)?;
    if is_managed_plugin(&targets.opencode_plugin) {
        fs::remove_file(&targets.opencode_plugin)
            .map_err(|_| failed(&targets.opencode_plugin, "it could not be removed"))?;
    }
    Ok(())
}

pub fn status(env: &Environment) -> Status {
    let targets = Targets::of(&env.home);
    let has_hook = |path: &Path| {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .is_some_and(|settings| managed_hook_groups(&settings).any(is_managed))
    };
    Status {
        claude: has_hook(&targets.claude_settings),
        codex: has_hook(&targets.codex_hooks),
        opencode: is_managed_plugin(&targets.opencode_plugin),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: &Path) -> Environment {
        Environment::new(home.to_path_buf(), home.to_path_buf(), [])
    }

    #[test]
    fn a_hook_is_added_once_and_updated_in_place() {
        let settings = json!({"theme": "dark", "hooks": {"SessionStart": [{"matcher": "x", "hooks": [{"type": "command", "command": "other"}]}]}});
        let added = with_hook(&settings, "hevy-axi").unwrap().unwrap();
        assert_eq!(added["theme"], "dark");
        assert_eq!(added["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
        assert_eq!(
            with_hook(&added, "hevy-axi").unwrap(),
            None,
            "already correct"
        );

        let moved = with_hook(&added, "/opt/hevy-axi").unwrap().unwrap();
        assert_eq!(moved["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
        assert_eq!(
            moved["hooks"]["SessionStart"][1]["hooks"][0]["command"],
            "/opt/hevy-axi"
        );
    }

    #[test]
    fn unexpected_shapes_are_refused_not_overwritten() {
        assert!(with_hook(&json!([]), "c").is_err());
        assert!(with_hook(&json!({"hooks": "x"}), "c").is_err());
        assert!(with_hook(&json!({"hooks": {"SessionStart": {}}}), "c").is_err());
    }

    #[test]
    fn removal_takes_only_our_entries_and_tidies_up() {
        let installed = with_hook(&json!({"hooks": {"SessionStart": [{"matcher": "", "hooks": [{"type": "command", "command": "other"}]}]}}), "hevy-axi").unwrap().unwrap();
        let removed = without_hook(&installed).unwrap();
        assert_eq!(
            removed,
            json!({"hooks": {"SessionStart": [{"matcher": "", "hooks": [{"type": "command", "command": "other"}]}]}})
        );
        assert_eq!(without_hook(&removed), None);

        let alone = with_hook(&json!({}), "hevy-axi").unwrap().unwrap();
        assert_eq!(without_hook(&alone).unwrap(), json!({}));

        let mixed = json!({"hooks": {"SessionStart": [{"hooks": [{"command": "hevy-axi"}, {"command": "keep"}]}]}});
        assert_eq!(
            without_hook(&mixed).unwrap(),
            json!({"hooks": {"SessionStart": [{"hooks": [{"command": "keep"}]}]}})
        );
    }

    #[test]
    fn install_status_and_uninstall_cover_every_agent() {
        let home = tempfile::tempdir().unwrap();
        let env = env(home.path());
        assert_eq!(
            status(&env),
            Status {
                claude: false,
                codex: false,
                opencode: false
            }
        );

        install(&env, Path::new("/opt/bin/hevy-axi")).unwrap();
        assert_eq!(
            status(&env),
            Status {
                claude: true,
                codex: true,
                opencode: true
            }
        );
        install(&env, Path::new("/opt/bin/hevy-axi")).unwrap();
        let config = fs::read_to_string(home.path().join(".codex/config.toml")).unwrap();
        assert_eq!(config, "[features]\nhooks = true\n");
        let plugin = fs::read_to_string(
            home.path()
                .join(".config/opencode/plugins")
                .join(PLUGIN_NAME),
        )
        .unwrap();
        assert!(plugin.contains("const command = \"/opt/bin/hevy-axi\";"));

        uninstall(&env).unwrap();
        assert_eq!(
            status(&env),
            Status {
                claude: false,
                codex: false,
                opencode: false
            }
        );
        assert_eq!(
            fs::read_to_string(home.path().join(".codex/config.toml")).unwrap(),
            config,
            "the shared switch stays on"
        );
        uninstall(&env).unwrap();
    }

    #[test]
    fn codex_config_keeps_its_content() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("config.toml");
        for (before, after) in [
            ("", "[features]\nhooks = true\n"),
            (
                "model = \"x\" # keep\n",
                "model = \"x\" # keep\n\n[features]\nhooks = true\n",
            ),
            (
                "[features]\nhooks = false # off\nother = 1\n",
                "[features]\nhooks = true # off\nother = 1\n",
            ),
            (
                "[features]\nother = 1\n\n[tools]\na = 1\n",
                "[features]\nother = 1\nhooks = true\n\n[tools]\na = 1\n",
            ),
        ] {
            fs::write(&path, before).unwrap();
            enable_codex_hooks(&path).unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), after, "{before:?}");
        }
        fs::write(&path, "[features]\nhooks = true\n").unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        enable_codex_hooks(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            modified,
            "already enabled: untouched"
        );
        fs::write(&path, "features = 1\n").unwrap();
        assert!(enable_codex_hooks(&path).is_err());
    }

    #[test]
    fn an_unmanaged_opencode_plugin_is_never_overwritten_or_removed() {
        let home = tempfile::tempdir().unwrap();
        let env = env(home.path());
        let plugin = home
            .path()
            .join(".config/opencode/plugins")
            .join(PLUGIN_NAME);
        fs::create_dir_all(plugin.parent().unwrap()).unwrap();
        fs::write(&plugin, "// mine\n").unwrap();
        assert!(install(&env, Path::new("/opt/bin/hevy-axi")).is_err());
        uninstall(&env).unwrap();
        assert_eq!(fs::read_to_string(&plugin).unwrap(), "// mine\n");
    }

    #[test]
    fn the_bare_name_is_used_only_when_path_resolves_to_this_executable() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("hevy-axi");
        fs::write(&exe, "").unwrap();
        let other = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([other.path(), dir.path()]).unwrap();
        assert_eq!(hook_command(&exe, path.to_str()), "hevy-axi");
        assert_eq!(
            hook_command(&exe, Some(other.path().to_str().unwrap())),
            exe.display().to_string()
        );
        assert_eq!(hook_command(&exe, None), exe.display().to_string());
        let link_dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(&exe, link_dir.path().join("hevy-axi")).unwrap();
        assert_eq!(
            hook_command(&exe, link_dir.path().to_str()),
            "hevy-axi",
            "a symlink to this executable counts"
        );
    }
}
