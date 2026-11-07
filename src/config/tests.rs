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
