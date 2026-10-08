//! The Hevy HTTP client.
//!
//! Safety properties, each covered by tests:
//! - the API key is only ever sent to the configured base URL, and a redirect
//!   is blocked before it can carry the key anywhere else;
//! - only GET is retried; POST and PUT are sent exactly once;
//! - response and error bodies are size-bounded, and the key is redacted from
//!   anything echoed back in an error.

use std::io;
use std::sync::LazyLock;
use std::thread::sleep;
use std::time::Duration;

use chrono::{DateTime, Utc};
use regex_lite::Regex;
use serde_json::{Map, Value, json};
use url::Url;

use crate::config::{BaseUrl, Environment, resolve};
use crate::error::{Code, Error, Result};
use crate::fsutil::read_capped;

const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 8 * 1024;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(2);

static SECONDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d+(?:\.\d+)?$").expect("valid pattern"));

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Get,
    Post,
    Put,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClientOptions {
    pub timeout: Duration,
    pub max_read_retries: u32,
    pub base_retry_delay: Duration,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(20),
            max_read_retries: 2,
            base_retry_delay: Duration::from_millis(250),
        }
    }
}

pub struct Client {
    agent: ureq::Agent,
    api_key: String,
    base_url: BaseUrl,
    options: ClientOptions,
}

/// Why one attempt did not produce a result.
enum Failure {
    Fatal(Error),
    /// Worth another attempt if any remain; `error` is what to report otherwise.
    Transient {
        error: Error,
        retry_after: Option<Duration>,
    },
}

impl Client {
    pub fn new(api_key: &str, base_url: BaseUrl, options: ClientOptions) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(options.timeout))
            // Never follow a redirect: no other host may receive the API key.
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .user_agent(concat!("hevy-axi/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            agent,
            api_key: api_key.to_owned(),
            base_url,
            options,
        }
    }

    /// A client for the configured account, or the error that says how to configure one.
    pub fn configured(env: &Environment) -> Result<Self> {
        let config = resolve(env)?;
        let api_key = config.require_api_key()?.to_owned();
        Ok(Self::new(
            &api_key,
            config.base_url,
            ClientOptions::default(),
        ))
    }

    pub fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.request(Method::Get, path, query, None)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(Method::Post, path, &[], Some(body))
    }

    pub fn put(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(Method::Put, path, &[], Some(body))
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Value> {
        if !path.starts_with("/v1/") {
            return Err(Error::validation(
                "Hevy API paths must begin with \"/v1/\".",
            ));
        }
        let mut url = Url::parse(&format!("{}{path}", self.base_url.as_str()))
            .map_err(|_| Error::validation("The Hevy API path is invalid."))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let payload =
            body.map(|body| serde_json::to_vec(body).expect("a JSON value always serializes"));

        let retries = if method == Method::Get {
            self.options.max_read_retries
        } else {
            0
        };
        let mut attempt = 0;
        loop {
            match self.attempt(method, &url, payload.as_deref()) {
                Ok(value) => return Ok(value),
                Err(Failure::Fatal(error)) => return Err(error),
                Err(Failure::Transient { error, .. }) if attempt >= retries => return Err(error),
                Err(Failure::Transient { retry_after, .. }) => {
                    sleep(retry_after.unwrap_or_else(|| self.backoff(attempt)));
                    attempt += 1;
                }
            }
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        (self.options.base_retry_delay * 2u32.saturating_pow(attempt)).min(MAX_BACKOFF)
    }

    fn attempt(
        &self,
        method: Method,
        url: &Url,
        payload: Option<&[u8]>,
    ) -> std::result::Result<Value, Failure> {
        let builder = ureq::http::Request::builder()
            .method(method.as_str())
            .uri(url.as_str())
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("api-key", &self.api_key);
        let sent = match payload {
            Some(bytes) => builder
                .body(bytes.to_vec())
                .map(|request| self.agent.run(request)),
            None => builder.body(()).map(|request| self.agent.run(request)),
        };
        // The request is built from validated parts; a failure here is a bug in them.
        let sent = sent.map_err(|_| {
            Failure::Fatal(Error::validation(
                "The Hevy API request could not be built.",
            ))
        })?;
        let mut response = sent.map_err(|error| self.transport_failure(method, &error))?;

        let status = response.status().as_u16();
        if (300..400).contains(&status) {
            // Never inspect Location or the body: redirect targets and payloads
            // are untrusted, and no redirect may receive the API key.
            return Err(Failure::Fatal(
                Error::new(
                    Code::UnsafeRedirect,
                    "The Hevy API request was redirected and was blocked.",
                )
                .with_details(json!({ "status": status })),
            ));
        }

        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(retry_delay);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(str::to_lowercase);
        let limit = if response.status().is_success() {
            MAX_RESPONSE_BYTES
        } else {
            MAX_ERROR_BYTES
        };
        let (bytes, truncated) = read_capped(response.body_mut().as_reader(), limit)
            .map_err(|error| self.read_failure(method, &error))?;
        let text = String::from_utf8_lossy(&bytes);
        let body = parse_body(&text, content_type.as_deref());

        if response.status().is_success() {
            return if truncated {
                Err(Failure::Fatal(
                    Error::new(Code::Api, "The Hevy API response was too large.").with_details(
                        json!({ "status": status, "maximumBytes": MAX_RESPONSE_BYTES }),
                    ),
                ))
            } else {
                Ok(body.unwrap_or(Value::Null))
            };
        }

        let error = self.http_error(status, body, truncated);
        if matches!(status, 429 | 502 | 503 | 504) {
            Err(Failure::Transient { error, retry_after })
        } else {
            Err(Failure::Fatal(error))
        }
    }

    fn http_error(&self, status: u16, body: Option<Value>, truncated: bool) -> Error {
        let body = body.map(|body| redact(body, &self.api_key));
        let mut details = Map::from_iter([("status".to_owned(), status.into())]);
        if let Some(body) = &body {
            details.insert("body".into(), body.clone());
        }
        if truncated {
            details.insert("bodyTruncated".into(), true.into());
        }

        let (code, message, suggestions) = describe_status(status);
        let upstream = body
            .as_ref()
            .and_then(|b| b.get("error"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty());
        let message = match upstream {
            Some(text) => format!("{message}: {text}"),
            None => format!("{message}."),
        };
        let mut error = Error::new(code, message).with_details(Value::Object(details));
        error.suggestions = suggestions.iter().map(|s| (*s).to_owned()).collect();
        error
    }

    fn transport_failure(&self, method: Method, error: &ureq::Error) -> Failure {
        if let ureq::Error::Timeout(_) = error {
            return Failure::Fatal(self.timeout_error(method));
        }
        let cause = network_cause(error);
        let mut network = Error::new(Code::Network, "The Hevy API request failed.");
        if let Some(cause) = cause {
            network = network.with_details(json!({ "cause": cause }));
        }
        network.suggestions = mutation_suggestions(method);
        Failure::Transient {
            error: network,
            retry_after: None,
        }
    }

    fn read_failure(&self, method: Method, error: &io::Error) -> Failure {
        if error.kind() == io::ErrorKind::TimedOut {
            return Failure::Fatal(self.timeout_error(method));
        }
        let mut network = Error::new(Code::Network, "The Hevy API request failed.");
        network.suggestions = mutation_suggestions(method);
        Failure::Transient {
            error: network,
            retry_after: None,
        }
    }

    fn timeout_error(&self, method: Method) -> Error {
        let mut error = Error::new(Code::Timeout, "The Hevy API request timed out.")
            .with_details(json!({ "timeoutMs": self.options.timeout.as_millis() as u64 }));
        error.suggestions = mutation_suggestions(method);
        error
    }
}

/// A mutation that fails mid-flight may still have been applied.
fn mutation_suggestions(method: Method) -> Vec<String> {
    match method {
        Method::Get => Vec::new(),
        Method::Post | Method::Put => vec![
            "The mutation outcome may be unknown. Inspect Hevy before manually retrying to avoid a duplicate or overwrite.".to_owned(),
        ],
    }
}

fn describe_status(status: u16) -> (Code, String, &'static [&'static str]) {
    let (code, message, suggestions): (_, _, &'static [&'static str]) = match status {
        400 => (
            Code::BadRequest,
            "Hevy rejected the request as invalid",
            &[],
        ),
        401 => (
            Code::Auth,
            "Hevy rejected the API credentials",
            &["Check the configured Hevy API key and account access."],
        ),
        // Hevy also answers 403 for account limits, which no key change fixes.
        403 => (
            Code::Forbidden,
            "Hevy refused the request",
            &[
                "Hevy uses 403 for account limits such as the routine or custom-exercise limit; see details.body.",
                "If every request returns 403, check the API key and the account's Hevy Pro access.",
            ],
        ),
        404 => (
            Code::NotFound,
            "The requested Hevy resource was not found",
            &[],
        ),
        409 => (
            Code::Conflict,
            "The request conflicts with existing Hevy data",
            &["The target may already exist; read the current Hevy state before retrying."],
        ),
        429 => (Code::RateLimited, "Hevy rate-limited the request", &[]),
        _ => {
            return (
                Code::Api,
                format!("The Hevy API returned HTTP {status}"),
                &[],
            );
        }
    };
    (code, message.to_owned(), suggestions)
}

/// The system error name behind a transport failure. Messages are never
/// reported because they can echo request data.
fn network_cause(error: &ureq::Error) -> Option<&'static str> {
    use io::ErrorKind::*;
    match error {
        ureq::Error::HostNotFound => Some("ENOTFOUND"),
        ureq::Error::Io(error) => match error.kind() {
            ConnectionRefused => Some("ECONNREFUSED"),
            ConnectionReset => Some("ECONNRESET"),
            ConnectionAborted => Some("ECONNABORTED"),
            NotConnected => Some("ENOTCONN"),
            BrokenPipe => Some("EPIPE"),
            AddrInUse => Some("EADDRINUSE"),
            AddrNotAvailable => Some("EADDRNOTAVAIL"),
            _ => None,
        },
        _ => None,
    }
}

/// A `Retry-After` header: delay-seconds or an HTTP date, capped.
fn retry_delay(value: &str) -> Option<Duration> {
    let value = value.trim();
    if SECONDS.is_match(value) {
        let seconds: f64 = value.parse().ok()?;
        return Some(
            Duration::try_from_secs_f64(seconds)
                .unwrap_or(MAX_RETRY_AFTER)
                .min(MAX_RETRY_AFTER),
        );
    }
    let date = DateTime::parse_from_rfc2822(value)
        .ok()?
        .with_timezone(&Utc);
    let wait = (date - Utc::now()).to_std().unwrap_or(Duration::ZERO);
    Some(wait.min(MAX_RETRY_AFTER))
}

/// A body is JSON only if it says so and parses; anything else stays text.
fn parse_body(text: &str, content_type: Option<&str>) -> Option<Value> {
    if text.is_empty() {
        return None;
    }
    let says_json =
        content_type.is_some_and(|t| t.contains("application/json") || t.contains("+json"));
    if says_json && let Ok(value) = serde_json::from_str(text) {
        return Some(value);
    }
    Some(Value::String(text.to_owned()))
}

/// Replace the secret wherever it appears, in values and in object keys.
fn redact(value: Value, secret: &str) -> Value {
    let scrub = |text: &str| {
        if secret.is_empty() {
            text.to_owned()
        } else {
            text.replace(secret, "[REDACTED]")
        }
    };
    match value {
        Value::String(text) => Value::String(scrub(&text)),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|item| redact(item, secret)).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, entry)| (scrub(&key), redact(entry, secret)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_accepts_seconds_and_dates_and_caps_them() {
        assert_eq!(retry_delay("2"), Some(Duration::from_secs(2)));
        assert_eq!(retry_delay(" 0.5 "), Some(Duration::from_millis(500)));
        assert_eq!(retry_delay("3600"), Some(MAX_RETRY_AFTER));
        assert_eq!(
            retry_delay("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(Duration::ZERO)
        );
        assert_eq!(retry_delay("soon"), None);
        assert_eq!(retry_delay("-1"), None);
    }

    #[test]
    fn bodies_are_json_only_when_declared_and_valid() {
        assert_eq!(parse_body("", Some("application/json")), None);
        assert_eq!(
            parse_body("{\"a\":1}", Some("application/json; charset=utf-8")),
            Some(json!({"a": 1}))
        );
        assert_eq!(
            parse_body("{\"a\":1}", Some("application/problem+json")),
            Some(json!({"a": 1}))
        );
        assert_eq!(
            parse_body("{\"a\":1}", Some("text/plain")),
            Some(json!("{\"a\":1}"))
        );
        assert_eq!(
            parse_body("{oops", Some("application/json")),
            Some(json!("{oops"))
        );
        assert_eq!(parse_body("x", None), Some(json!("x")));
    }

    #[test]
    fn redaction_reaches_keys_and_nested_values() {
        let value = json!({"k-secret": ["a secret b", {"secret": 1}], "n": 2});
        assert_eq!(
            redact(value, "secret"),
            json!({"k-[REDACTED]": ["a [REDACTED] b", {"[REDACTED]": 1}], "n": 2})
        );
        assert_eq!(redact(json!("x"), ""), json!("x"));
    }

    #[test]
    fn backoff_doubles_up_to_a_ceiling() {
        let client = Client::new(
            "k",
            BaseUrl::parse("https://x.example").unwrap(),
            ClientOptions::default(),
        );
        let delays: Vec<u128> = (0..5).map(|n| client.backoff(n).as_millis()).collect();
        assert_eq!(delays, [250, 500, 1000, 2000, 2000]);
    }

    /// A server that accepts a request and then takes `delay` to answer it.
    fn slow_server(delay: Duration) -> (String, std::thread::JoinHandle<()>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr().to_ip().unwrap());
        let handle = std::thread::spawn(move || {
            if let Ok(request) = server.recv() {
                std::thread::sleep(delay);
                request.respond(tiny_http::Response::from_string("{}")).ok();
            }
        });
        (url, handle)
    }

    #[test]
    fn a_slow_server_times_out_without_retrying_and_a_write_warns_about_it() {
        let options = ClientOptions {
            timeout: Duration::from_millis(150),
            ..ClientOptions::default()
        };
        for (is_write, expect_advice) in [(false, false), (true, true)] {
            let (url, server) = slow_server(Duration::from_millis(700));
            let client = Client::new("k", BaseUrl::parse(&url).unwrap(), options.clone());
            let started = std::time::Instant::now();
            let error = if is_write {
                client.post("/v1/workouts", &json!({})).unwrap_err()
            } else {
                client.get("/v1/workouts", &[]).unwrap_err()
            };
            assert_eq!(error.code, Code::Timeout);
            assert_eq!(error.details, Some(json!({ "timeoutMs": 150 })));
            assert_eq!(!error.suggestions.is_empty(), expect_advice);
            assert!(
                started.elapsed() < Duration::from_millis(600),
                "a timeout is final, not retried"
            );
            server.join().unwrap();
        }
    }
}
