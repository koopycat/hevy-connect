//! A black-box harness: the real `hevy-axi` binary against a mock Hevy server,
//! in a private home and project directory.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub target: String,
    pub body: String,
    pub headers: HashMap<String, String>,
}

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
}

impl Reply {
    pub fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", "application/json".into())],
            body: value.to_string(),
        }
    }

    pub fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", "text/plain".into())],
            body: body.to_owned(),
        }
    }

    pub fn with_header(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.to_owned()));
        self
    }
}

type Handler = dyn Fn(&Recorded) -> Reply + Send + Sync + 'static;

pub struct Mock {
    server: Arc<tiny_http::Server>,
    thread: Option<JoinHandle<()>>,
    requests: Arc<Mutex<Vec<Recorded>>>,
    pub url: String,
}

impl Mock {
    pub fn start(handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Self {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("bind a local port"));
        let url = format!(
            "http://{}",
            server.server_addr().to_ip().expect("an IP address")
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Handler> = Arc::new(handler);

        let thread = {
            let (server, requests) = (server.clone(), requests.clone());
            std::thread::spawn(move || {
                for mut request in server.incoming_requests() {
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).ok();
                    let headers = request
                        .headers()
                        .iter()
                        .map(|h| {
                            (
                                h.field.as_str().as_str().to_lowercase(),
                                h.value.to_string(),
                            )
                        })
                        .collect();
                    let recorded = Recorded {
                        method: request.method().to_string(),
                        target: request.url().to_owned(),
                        body,
                        headers,
                    };
                    let reply = handler(&recorded);
                    requests.lock().unwrap().push(recorded);
                    let mut response =
                        tiny_http::Response::from_string(reply.body).with_status_code(reply.status);
                    for (name, value) in reply.headers {
                        response.add_header(tiny_http::Header::from_bytes(name, value).unwrap());
                    }
                    request.respond(response).ok();
                }
            })
        };
        Self {
            server,
            thread: Some(thread),
            requests,
            url,
        }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }

    pub fn targets(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|r| format!("{} {}", r.method, r.target))
            .collect()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

pub struct Output {
    pub stdout: String,
    pub code: i32,
}

impl Output {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|error| panic!("not JSON ({error}): {}", self.stdout))
    }
}

/// A sandbox with its own `HOME` and working directory, and a configured key.
pub struct Cli {
    dir: tempfile::TempDir,
    env: Vec<(String, String)>,
}

pub const KEY: &str = "test-secret-api-key-never-print";

impl Cli {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("home")).unwrap();
        std::fs::create_dir(dir.path().join("project")).unwrap();
        Self {
            dir,
            env: vec![("HEVY_API_KEY".into(), KEY.into())],
        }
    }

    pub fn against(mock: &Mock) -> Self {
        Self::new().with_env("HEVY_API_BASE_URL", &mock.url)
    }

    pub fn with_env(mut self, name: &str, value: &str) -> Self {
        self.env.retain(|(k, _)| k != name);
        self.env.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn without_key(mut self) -> Self {
        self.env.retain(|(k, _)| k != "HEVY_API_KEY");
        self
    }

    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    pub fn project(&self) -> PathBuf {
        self.dir.path().join("project")
    }

    /// Write a file under the project directory with the given mode.
    pub fn file(&self, name: &str, contents: &str, mode: u32) -> PathBuf {
        let path = self.project().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.run_with_stdin(args, "")
    }

    pub fn run_with_stdin(&self, args: &[&str], stdin: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hevy-axi"));
        command
            .args(args)
            .current_dir(self.project())
            .env_clear()
            .env("HOME", self.home())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn hevy-axi");
        let mut input = child.stdin.take().unwrap();
        let stdin = stdin.to_owned();
        let writer = std::thread::spawn(move || input.write_all(stdin.as_bytes()).ok());
        let output = child.wait_with_output().expect("hevy-axi exits");
        writer.join().ok();
        assert!(
            output.stderr.is_empty(),
            "stderr must stay empty: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Output {
            stdout: String::from_utf8(output.stdout).expect("UTF-8 output"),
            code: output.status.code().expect("exit code"),
        }
    }
}

pub fn path_exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}
