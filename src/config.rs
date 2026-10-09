//! Credential and base-URL resolution, and the globally stored API key.
//!
//! The key comes from `HEVY_API_KEY`, or else from
//! `~/.config/hevy-axi/credentials.env`, which only `setup key` writes. The
//! base URL comes from `HEVY_API_BASE_URL` alone, so no file can redirect a key.

use std::collections::HashMap;
use std::fs::{self, DirBuilder};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

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
        // Variables that are not valid UTF-8 cannot be ours, so they are skipped
        // instead of aborting the process.
        let vars = std::env::vars_os().filter_map(|(name, value)| {
            Some((name.into_string().ok()?, value.into_string().ok()?))
        });
        Ok(Self::new(cwd, home, vars))
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

    pub fn stored_credentials_path(&self) -> PathBuf {
        self.home.join(STORED_CREDENTIALS)
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Environment,
    Stored,
}

pub struct Config {
    pub api_key: Option<String>,
    pub base_url: BaseUrl,
    pub source: Option<Source>,
}

impl Config {
    /// A description of the key's origin that never reveals a path.
    pub fn credential_category(&self) -> Option<&'static str> {
        self.source.map(|source| match source {
            Source::Environment => "environment",
            Source::Stored => "global",
        })
    }

    pub fn require_api_key(&self) -> Result<&str> {
        self.api_key.as_deref().ok_or_else(|| {
            Error::validation("A Hevy API key is required.")
                .with_suggestion("Set HEVY_API_KEY or run the credential configuration command.")
        })
    }
}

/// The key in the contents of the stored file: the `HEVY_API_KEY=` line that
/// `store_api_key` writes, double-quoted with `\\` and `\"` escapes, or bare.
/// `None` means the value is malformed; a file without the line has no key.
fn parse_stored_key(contents: &str) -> Option<Option<String>> {
    let Some(raw) = contents
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("HEVY_API_KEY"))
        .and_then(|rest| rest.trim_start().strip_prefix('='))
    else {
        return Some(None);
    };
    let raw = raw.trim();
    let Some(quoted) = raw.strip_prefix('"') else {
        return Some(Some(raw.to_owned()));
    };
    let quoted = quoted.strip_suffix('"')?;
    let mut key = String::with_capacity(quoted.len());
    let mut chars = quoted.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(&next @ ('"' | '\\'))) => {
                key.push(next);
                chars.next();
            }
            (c, _) => key.push(c),
        }
    }
    Some(Some(key))
}

/// Read the key from the stored file. `None` means there is no such file.
fn read_stored_key(path: &Path) -> Result<Option<String>> {
    let file = match open_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
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
    if !metadata.is_file() {
        return Err(Error::insecure(
            "The credential path must be a regular, non-symbolic-link file.",
            path,
        ));
    }
    if metadata.mode() & 0o077 != 0 || metadata.uid() != rustix::process::getuid().as_raw() {
        return Err(Error::insecure(
            "The credential file must be owned by the current user with mode 0600.",
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

    let invalid = || Error::config("A credential file contains an invalid value.", path);
    let key = parse_stored_key(&String::from_utf8_lossy(&bytes)).ok_or_else(invalid)?;
    match key.filter(|key| !key.trim().is_empty()) {
        Some(key) if has_control_characters(&key) => Err(Error::config(
            "A credential file contains an invalid API key.",
            path,
        )),
        key => Ok(key.map(|key| key.trim().to_owned())),
    }
}

fn has_control_characters(value: &str) -> bool {
    value.contains(['\r', '\n', '\0'])
}

/// Resolve the API key and base URL. The key itself is never part of any
/// error or source description.
pub fn resolve(env: &Environment) -> Result<Config> {
    let base_url = env.setting("HEVY_API_BASE_URL");
    let base_url = BaseUrl::parse(base_url.as_deref().unwrap_or(DEFAULT_BASE_URL))?;

    // A key from the environment never consults the stored file.
    if let Some(api_key) = env.setting("HEVY_API_KEY") {
        if has_control_characters(&api_key) {
            return Err(Error::new(
                Code::Config,
                "The HEVY_API_KEY environment value is invalid.",
            ));
        }
        return Ok(Config {
            api_key: Some(api_key),
            base_url,
            source: Some(Source::Environment),
        });
    }
    let api_key = read_stored_key(&env.stored_credentials_path())?;
    Ok(Config {
        source: api_key.as_ref().map(|_| Source::Stored),
        api_key,
        base_url,
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
    fn stored_key_lines() {
        for (contents, want) in [
            ("HEVY_API_KEY=\"k\\\"ey\\\\\"\n", Some(Some("k\"ey\\"))),
            ("HEVY_API_KEY = abc\n", Some(Some("abc"))),
            ("HEVY_API_KEY=\"a\\qb\"", Some(Some("a\\qb"))),
            ("OTHER=1\n", Some(None)),
            ("HEVY_API_KEY=\"unterminated\n", None),
        ] {
            assert_eq!(
                parse_stored_key(contents),
                want.map(|key| key.map(str::to_owned)),
                "{contents}"
            );
        }
    }

    #[test]
    fn an_environment_key_never_reads_the_stored_file() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[
            ("HEVY_API_KEY", " env-key "),
            ("HEVY_API_BASE_URL", "http://127.0.0.1:9"),
        ]);
        // Even an insecure stored file is not consulted.
        write(
            &env.stored_credentials_path(),
            "HEVY_API_KEY=file-key\n",
            0o644,
        );
        let config = resolve(&env).unwrap();
        assert_eq!(config.api_key.as_deref(), Some("env-key"));
        assert_eq!(config.base_url.as_str(), "http://127.0.0.1:9");
        assert_eq!(config.credential_category(), Some("environment"));
    }

    #[test]
    fn the_stored_key_is_used_with_the_environment_base_url() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        let config = resolve(&env).unwrap();
        assert_eq!(
            (config.api_key.as_deref(), config.credential_category()),
            (None, None)
        );

        write(
            &env.stored_credentials_path(),
            "HEVY_API_KEY=global\n",
            0o600,
        );
        let config = resolve(&env).unwrap();
        assert_eq!(
            (config.api_key.as_deref(), config.credential_category()),
            (Some("global"), Some("global"))
        );
        assert_eq!(config.base_url.as_str(), DEFAULT_BASE_URL);

        let env = sandbox.env(&[("HEVY_API_BASE_URL", "https://proxy.example/")]);
        assert_eq!(
            resolve(&env).unwrap().base_url.as_str(),
            "https://proxy.example"
        );
    }

    #[test]
    fn an_insecure_stored_file_is_refused() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        let path = env.stored_credentials_path();
        // The mode is checked whatever the file contains.
        write(&path, "OTHER=1\n", 0o640);
        assert_eq!(resolve(&env).err().unwrap().code, Code::ConfigInsecure);

        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(resolve(&env).err().unwrap().code, Code::ConfigInsecure);

        fs::remove_dir(&path).unwrap();
        let target = sandbox.dir.path().join("target.env");
        write(&target, "HEVY_API_KEY=k\n", 0o600);
        std::os::unix::fs::symlink(&target, &path).unwrap();
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
    fn bad_values_are_reported_without_the_secret() {
        let sandbox = Sandbox::new();
        let env = sandbox.env(&[]);
        write(
            &env.stored_credentials_path(),
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
