//! Credential and base-URL resolution, and the globally stored API key.
//!
//! Precedence for the key: `HEVY_API_KEY`, then the file named by
//! `HEVY_AXI_ENV_FILE`, then `./.env`, then `~/.config/hevy-axi/credentials.env`.
//! A key from the environment never consults a file; a key from a file is
//! bound to the base URL in that same file.

use std::collections::HashMap;
use std::fs::{self, DirBuilder};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex_lite::Regex;
use url::Url;

use crate::error::{Code, Error, Result};
use crate::fsutil::{is_symlink_error, open_no_follow, read_capped};

pub const DEFAULT_BASE_URL: &str = "https://api.hevyapp.com";
const STORED_CREDENTIALS: &str = ".config/hevy-axi/credentials.env";
const MAX_CREDENTIAL_FILE_BYTES: usize = 64 * 1024;

/// The process facts configuration depends on, captured once so resolution
/// stays a pure function of them.
pub struct Environment {
    pub cwd: PathBuf,
    pub home: PathBuf,
    vars: HashMap<String, String>,
}

impl Environment {
    pub fn from_process() -> io::Result<Self> {
        let cwd = std::env::current_dir()?;
        let home = std::env::home_dir().ok_or_else(|| io::Error::other("no home directory"))?;
        Ok(Self::new(cwd, home, std::env::vars()))
    }

    pub fn new(
        cwd: PathBuf,
        home: PathBuf,
        vars: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        Self {
            cwd,
            home,
            vars: vars.into_iter().collect(),
        }
    }

    /// The trimmed value of a variable, or `None` if unset or blank.
    pub fn setting(&self, name: &str) -> Option<String> {
        nonblank(self.vars.get(name).map(String::as_str))
    }

    pub fn path_variable(&self) -> Option<&str> {
        self.vars.get("PATH").map(String::as_str)
    }

    pub fn stored_credentials_path(&self) -> PathBuf {
        self.home.join(STORED_CREDENTIALS)
    }

    pub fn project_credentials_path(&self) -> PathBuf {
        self.cwd.join(".env")
    }
}

fn nonblank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// The HTTPS (or local test-server) root every request is made under.
#[derive(Clone, Debug)]
pub struct BaseUrl(String);

impl BaseUrl {
    pub fn parse(value: &str) -> Result<Self> {
        let invalid = |message: &str| Error::new(Code::Config, message);
        let url = Url::parse(value).map_err(|_| invalid("The Hevy API base URL is invalid."))?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(invalid(
                "The Hevy API base URL must not contain user information.",
            ));
        }
        if url.query().is_some_and(|q| !q.is_empty())
            || url.fragment().is_some_and(|f| !f.is_empty())
        {
            return Err(invalid(
                "The Hevy API base URL must not contain a query or fragment.",
            ));
        }
        let local_http = url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if url.scheme() != "https" && !local_http {
            return Err(invalid(
                "The Hevy API base URL must use HTTPS (except for a local test server).",
            ));
        }
        Ok(Self(url.as_str().trim_end_matches('/').to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where the API key came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Environment,
    File(PathBuf),
}

pub struct Config {
    pub api_key: Option<String>,
    pub base_url: BaseUrl,
    pub source: Option<Source>,
}

impl Config {
    /// A description of the key's origin that never reveals a path.
    pub fn credential_category(&self, env: &Environment) -> Option<&'static str> {
        match self.source.as_ref()? {
            Source::Environment => Some("environment"),
            Source::File(path) if *path == env.project_credentials_path() => Some("project"),
            Source::File(path) if *path == env.stored_credentials_path() => Some("global"),
            Source::File(_) => Some("explicit"),
        }
    }

    pub fn require_api_key(&self) -> Result<&str> {
        self.api_key.as_deref().ok_or_else(|| {
            Error::validation("A Hevy API key is required.")
                .with_suggestion("Set HEVY_API_KEY or run the credential configuration command.")
        })
    }
}

#[derive(Default)]
struct Settings {
    api_key: Option<String>,
    base_url: Option<String>,
}

static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:export\s+)?(HEVY_API_KEY|HEVY_API_BASE_URL)\s*=\s*(.*)$")
        .expect("valid pattern")
});

/// One dotenv value: quoted (with escapes in double quotes) or bare, with an
/// optional trailing comment. `None` means the value is malformed.
fn dotenv_value(raw: &str) -> Option<String> {
    let value = raw.trim();
    let Some(quote) = value.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
        let end = value.find('#').unwrap_or(value.len());
        return Some(value[..end].trim().to_owned());
    };

    let body = &value[1..];
    let mut escaped = false;
    let mut closing = None;
    for (index, c) in body.char_indices() {
        if quote == '"' && c == '\\' && !escaped {
            escaped = true;
            continue;
        }
        if c == quote && !escaped {
            closing = Some(index);
            break;
        }
        escaped = false;
    }
    let closing = closing?;
    let rest = body[closing + 1..].trim();
    if !rest.is_empty() && !rest.starts_with('#') {
        return None;
    }

    let quoted = &body[..closing];
    if quote == '\'' {
        return Some(quoted.to_owned());
    }
    let mut unescaped = String::with_capacity(quoted.len());
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            unescaped.push(c);
            continue;
        }
        match chars.clone().next() {
            Some('n') => unescaped.push('\n'),
            Some('r') => unescaped.push('\r'),
            Some('t') => unescaped.push('\t'),
            Some(next @ ('"' | '\\')) => unescaped.push(next),
            _ => {
                unescaped.push('\\');
                continue;
            }
        }
        chars.next();
    }
    Some(unescaped)
}

fn parse_env_file(contents: &str, path: &Path) -> Result<Settings> {
    let mut settings = Settings::default();
    for line in contents.lines() {
        let Some(captures) = ASSIGNMENT.captures(line) else {
            continue;
        };
        let value = dotenv_value(&captures[2])
            .ok_or_else(|| Error::config("A credential file contains an invalid value.", path))?;
        if &captures[1] == "HEVY_API_KEY" {
            settings.api_key = Some(value);
        } else {
            settings.base_url = Some(value);
        }
    }
    Ok(settings)
}

struct Candidate {
    path: PathBuf,
    required: bool,
}

/// Read one credential file safely. `None` means there is nothing to read
/// (an optional file that does not exist, or a `.env` directory).
fn read_candidate(candidate: &Candidate) -> Result<Option<Settings>> {
    let path = &candidate.path;
    let file = match open_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound && !candidate.required => {
            return Ok(None);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(Error::config(
                "The configured credential file does not exist.",
                path,
            ));
        }
        Err(error) if is_symlink_error(&error) => {
            return Err(Error::insecure(
                "The credential path must not be a symbolic link.",
                path,
            ));
        }
        Err(_) => {
            return Err(Error::config(
                "The credential file could not be opened safely.",
                path,
            ));
        }
    };

    let unreadable = || Error::config("The credential file could not be read safely.", path);
    let too_large = || Error::config("The credential file exceeds the 64 KiB size limit.", path);
    let metadata = file.metadata().map_err(|_| unreadable())?;
    // A project `.env` directory, such as a Python virtualenv, cannot hold
    // credentials and must not block the remaining sources.
    if !candidate.required && metadata.is_dir() {
        return Ok(None);
    }
    if !metadata.is_file() {
        return Err(Error::insecure(
            "The credential path must be a regular, non-symbolic-link file.",
            path,
        ));
    }
    if metadata.len() > MAX_CREDENTIAL_FILE_BYTES as u64 {
        return Err(too_large());
    }
    let (bytes, more) = read_capped(&file, MAX_CREDENTIAL_FILE_BYTES).map_err(|_| unreadable())?;
    if more {
        return Err(too_large());
    }

    let settings = parse_env_file(&String::from_utf8_lossy(&bytes), path)?;
    // Ownership and mode protect Hevy settings only. A file that sets neither
    // variable, such as an unrelated project .env, is ignored whatever its mode.
    let configures_hevy = settings.api_key.is_some() || settings.base_url.is_some();
    let foreign_owner = metadata.uid() != rustix::process::getuid().as_raw();
    if configures_hevy && (metadata.mode() & 0o077 != 0 || foreign_owner) {
        return Err(Error::insecure(
            "The credential file must be owned by the current user with mode 0600.",
            path,
        ));
    }
    Ok(Some(settings))
}

fn expand_path(path: &str, env: &Environment) -> PathBuf {
    if path == "~" {
        env.home.clone()
    } else if let Some(rest) = path.strip_prefix("~/") {
        env.home.join(rest)
    } else {
        // Joining an absolute path replaces the base.
        env.cwd.join(path)
    }
}

fn has_control_characters(value: &str) -> bool {
    value.contains(['\r', '\n', '\0'])
}

/// Resolve the API key and base URL. The key itself is never part of any
/// error or source description.
pub fn resolve(env: &Environment) -> Result<Config> {
    let mut api_key = env.setting("HEVY_API_KEY");
    let mut source = api_key.as_ref().map(|_| Source::Environment);
    let env_base_url = env.setting("HEVY_API_BASE_URL");
    let mut base_url = env_base_url.clone();

    if api_key.as_deref().is_some_and(has_control_characters) {
        return Err(Error::new(
            Code::Config,
            "The HEVY_API_KEY environment value is invalid.",
        ));
    }

    // A directly supplied key is intentionally independent of every credential
    // file. In particular, a local .env must not be able to redirect it.
    if api_key.is_none() {
        let mut candidates = Vec::new();
        if let Some(file) = env.setting("HEVY_AXI_ENV_FILE") {
            candidates.push(Candidate {
                path: expand_path(&file, env),
                required: true,
            });
        }
        candidates.push(Candidate {
            path: env.project_credentials_path(),
            required: false,
        });
        candidates.push(Candidate {
            path: env.stored_credentials_path(),
            required: false,
        });

        let mut unbound_base_url = None;
        let mut seen: Vec<&Path> = Vec::new();
        for candidate in &candidates {
            if seen.contains(&candidate.path.as_path()) {
                continue;
            }
            seen.push(&candidate.path);

            let Some(settings) = read_candidate(candidate)? else {
                continue;
            };
            let file_base_url = nonblank(settings.base_url.as_deref());
            unbound_base_url = unbound_base_url.or_else(|| file_base_url.clone());
            let Some(key) = nonblank(settings.api_key.as_deref()) else {
                continue;
            };
            if has_control_characters(&key) {
                return Err(Error::config(
                    "A credential file contains an invalid API key.",
                    &candidate.path,
                ));
            }
            api_key = Some(key);
            source = Some(Source::File(candidate.path.clone()));
            // The environment base URL is an explicit override. Otherwise a file
            // key is bound only to a base URL from that exact same file.
            base_url = env_base_url.clone().or(file_base_url);
            break;
        }
        if api_key.is_none() && base_url.is_none() {
            base_url = unbound_base_url;
        }
    }

    Ok(Config {
        api_key,
        base_url: BaseUrl::parse(base_url.as_deref().unwrap_or(DEFAULT_BASE_URL))?,
        source,
    })
}

fn target_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(Error::insecure(
            "The stored credential target must be a regular, non-symbolic-link file.",
            path,
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Error::config(
            "The stored credential target could not be inspected.",
            path,
        )),
    }
}

fn secure_directory(path: &Path) -> Result<()> {
    let unsecured = || Error::config("The credential directory could not be secured.", path);
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|_| unsecured())?;
    let metadata = fs::symlink_metadata(path).map_err(|_| unsecured())?;
    if !metadata.is_dir() {
        return Err(Error::insecure(
            "The credential directory must be a non-symbolic-link directory.",
            path,
        ));
    }
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|_| unsecured())
}

/// Store the key atomically at `~/.config/hevy-axi/credentials.env` (mode
/// 0600, directory 0700). Returns `"created"` or `"updated"`.
pub fn store_api_key(env: &Environment, api_key: &str) -> Result<&'static str> {
    let key = api_key.trim();
    if key.is_empty() || has_control_characters(key) {
        return Err(Error::validation("The Hevy API key is invalid."));
    }
    let path = env.stored_credentials_path();
    let directory = path.parent().expect("the stored path has a parent");
    secure_directory(directory)?;
    let existed = target_exists(&path)?;

    let failed = || Error::config("The API key could not be stored.", &path);
    // A private temporary file in the same directory, renamed into place, so a
    // crash never leaves a partial or world-readable credential file.
    let mut file = tempfile::Builder::new()
        .prefix(".credentials.env.")
        .suffix(".tmp")
        .tempfile_in(directory)
        .map_err(|_| failed())?;
    let quoted = key.replace('\\', "\\\\").replace('"', "\\\"");
    writeln!(file, "HEVY_API_KEY=\"{quoted}\"").map_err(|_| failed())?;
    file.as_file().sync_all().map_err(|_| failed())?;
    file.persist(&path).map_err(|_| failed())?;
    Ok(if existed { "updated" } else { "created" })
}

/// Remove the stored key. Returns `"removed"` or `"not_found"`.
pub fn remove_stored_api_key(env: &Environment) -> Result<&'static str> {
    let path = env.stored_credentials_path();
    if !target_exists(&path)? {
        return Ok("not_found");
    }
    match fs::remove_file(&path) {
        Ok(()) => Ok("removed"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("not_found"),
        Err(_) => Err(Error::config(
            "The stored API key could not be removed.",
            &path,
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    struct Sandbox {
        dir: tempfile::TempDir,
    }

    impl Sandbox {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn env(&self, vars: &[(&str, &str)]) -> Environment {
            let cwd = self.dir.path().join("project");
            fs::create_dir_all(&cwd).unwrap();
            let home = self.dir.path().join("home");
            fs::create_dir_all(&home).unwrap();
            Environment::new(
                cwd,
                home,
                vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
            )
        }
    }

    fn write(path: &Path, contents: &str, mode: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn dotenv_values() {
        for (raw, want) in [
            ("abc", Some("abc")),
            ("  abc  # note", Some("abc")),
            ("", Some("")),
            ("\"a b\"", Some("a b")),
            ("\"a\\\"b\\n\\\\\"  # c", Some("a\"b\n\\")),
            ("\"a\\qb\"", Some("a\\qb")),
            ("'a\\nb'", Some("a\\nb")),
            ("'a' # c", Some("a")),
            ("\"unterminated", None),
            ("\"a\" trailing", None),
        ] {
            assert_eq!(dotenv_value(raw).as_deref(), want, "{raw}");
        }
    }

    #[test]
    fn environment_key_wins_and_ignores_files() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[
            ("HEVY_API_KEY", " env-key "),
            ("HEVY_API_BASE_URL", "http://127.0.0.1:9"),
        ]);
        write(
            &env.project_credentials_path(),
            "HEVY_API_KEY=file-key\nHEVY_API_BASE_URL=https://evil.example\n",
            0o644,
        );
        let config = resolve(&env).unwrap();
        assert_eq!(config.api_key.as_deref(), Some("env-key"));
        assert_eq!(config.base_url.as_str(), "http://127.0.0.1:9");
        assert_eq!(config.credential_category(&env), Some("environment"));
    }

    #[test]
    fn files_are_consulted_in_precedence_order() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        write(
            &env.stored_credentials_path(),
            "HEVY_API_KEY=global\n",
            0o600,
        );
        assert_eq!(
            resolve(&env).unwrap().credential_category(&env),
            Some("global")
        );
        write(
            &env.project_credentials_path(),
            "export HEVY_API_KEY='project'\n",
            0o600,
        );
        let config = resolve(&env).unwrap();
        assert_eq!(
            (config.api_key.as_deref(), config.credential_category(&env)),
            (Some("project"), Some("project"))
        );

        let explicit = sandbox.dir.path().join("explicit.env");
        write(&explicit, "HEVY_API_KEY=explicit\n", 0o600);
        let env = sandbox.env(&[("HEVY_AXI_ENV_FILE", explicit.to_str().unwrap())]);
        let config = resolve(&env).unwrap();
        assert_eq!(
            (config.api_key.as_deref(), config.credential_category(&env)),
            (Some("explicit"), Some("explicit"))
        );
    }

    #[test]
    fn a_file_key_is_bound_to_its_own_base_url() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        write(&env.project_credentials_path(), "HEVY_API_KEY=k\n", 0o600);
        write(
            &env.stored_credentials_path(),
            "HEVY_API_KEY=other\nHEVY_API_BASE_URL=https://other.example\n",
            0o600,
        );
        assert_eq!(resolve(&env).unwrap().base_url.as_str(), DEFAULT_BASE_URL);

        // A base URL with no key anywhere is still honoured.
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        write(
            &env.stored_credentials_path(),
            "HEVY_API_BASE_URL=https://proxy.example/\n",
            0o600,
        );
        let config = resolve(&env).unwrap();
        assert_eq!(
            (config.api_key, config.base_url.as_str()),
            (None, "https://proxy.example")
        );
    }

    #[test]
    fn insecure_credential_files_are_refused() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        write(&env.project_credentials_path(), "HEVY_API_KEY=k\n", 0o640);
        assert_eq!(resolve(&env).err().unwrap().code, Code::ConfigInsecure);

        // An unrelated .env is ignored whatever its mode.
        write(&env.project_credentials_path(), "OTHER=1\n", 0o644);
        assert!(resolve(&env).unwrap().api_key.is_none());

        // A .env directory is skipped.
        fs::remove_file(env.project_credentials_path()).unwrap();
        fs::create_dir(env.project_credentials_path()).unwrap();
        assert!(resolve(&env).unwrap().api_key.is_none());

        // Symbolic links are refused, wherever they point.
        fs::remove_dir(env.project_credentials_path()).unwrap();
        let target = sandbox.dir.path().join("target.env");
        write(&target, "HEVY_API_KEY=k\n", 0o600);
        std::os::unix::fs::symlink(&target, env.project_credentials_path()).unwrap();
        let error = resolve(&env).err().unwrap();
        assert_eq!(
            (error.code, error.message.as_str()),
            (
                Code::ConfigInsecure,
                "The credential path must not be a symbolic link."
            )
        );
    }

    #[test]
    fn a_named_credential_file_must_exist() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[("HEVY_AXI_ENV_FILE", "missing.env")]);
        let error = resolve(&env).err().unwrap();
        assert_eq!(
            error.message,
            "The configured credential file does not exist."
        );
    }

    #[test]
    fn bad_values_are_reported_without_the_secret() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        write(
            &env.project_credentials_path(),
            "HEVY_API_KEY=\"secret-never-shown\n",
            0o600,
        );
        let error = resolve(&env).err().unwrap();
        assert_eq!(
            error.message,
            "A credential file contains an invalid value."
        );
        assert!(!format!("{error:?}").contains("secret-never-shown"));

        let env = sandbox.env(&[("HEVY_API_KEY", "a\nb")]);
        assert_eq!(
            resolve(&env).err().unwrap().message,
            "The HEVY_API_KEY environment value is invalid."
        );
    }

    #[test]
    fn base_urls_must_be_https_or_local_and_plain() {
        for good in [
            "https://api.hevyapp.com",
            "https://proxy.example/v1/",
            "http://localhost:8080",
            "http://127.0.0.1:1",
            "http://[::1]:1",
        ] {
            assert!(BaseUrl::parse(good).is_ok(), "{good}");
        }
        for bad in [
            "http://example.com",
            "ftp://x",
            "not a url",
            "https://u:p@x.example",
            "https://x.example/?q=1",
            "https://x.example/#f",
            "localhost:8080",
        ] {
            assert!(BaseUrl::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(
            BaseUrl::parse("https://x.example/v1///").unwrap().as_str(),
            "https://x.example/v1"
        );
    }

    #[test]
    fn stored_keys_are_written_privately_and_removed() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        assert_eq!(store_api_key(&env, " k\"ey\\ ").unwrap(), "created");
        assert_eq!(store_api_key(&env, "second").unwrap(), "updated");
        let path = env.stored_credentials_path();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(resolve(&env).unwrap().api_key.as_deref(), Some("second"));
        assert_eq!(
            fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1,
            "no temporary file is left behind"
        );

        store_api_key(&env, "k\"ey\\").unwrap();
        assert_eq!(resolve(&env).unwrap().api_key.as_deref(), Some("k\"ey\\"));

        assert_eq!(remove_stored_api_key(&env).unwrap(), "removed");
        assert_eq!(remove_stored_api_key(&env).unwrap(), "not_found");
        assert_eq!(
            store_api_key(&env, "  ").err().unwrap().code,
            Code::Validation
        );
    }

    #[test]
    fn storing_refuses_a_symlinked_target() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        let path = env.stored_credentials_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let elsewhere = sandbox.dir.path().join("elsewhere");
        fs::write(&elsewhere, "keep").unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        assert_eq!(
            store_api_key(&env, "k").err().unwrap().code,
            Code::ConfigInsecure
        );
        assert_eq!(
            remove_stored_api_key(&env).err().unwrap().code,
            Code::ConfigInsecure
        );
        assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "keep");
    }
}
