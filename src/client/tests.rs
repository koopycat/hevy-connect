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
    for bad in ["-1", ".5", "5.", "1.2.3", "1e3", ""] {
        assert_eq!(retry_delay(bad), None, "{bad:?}");
    }
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
        let (url, server) = slow_server(Duration::from_millis(1600));
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
            started.elapsed() < Duration::from_millis(1400),
            "a timeout is final, not retried"
        );
        server.join().unwrap();
    }
}

#[test]
fn a_body_that_stalls_midway_is_a_timeout_and_is_not_retried() {
    for framing in ["Content-Length: 100", "Transfer-Encoding: chunked"] {
        stalled_body_times_out(framing);
    }
}

fn stalled_body_times_out(framing: &'static str) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 1024];
        assert!(
            stream.read(&mut request).unwrap() > 0,
            "the client sent a request"
        );
        let head =
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{framing}\r\n\r\n");
        stream.write_all(head.as_bytes()).unwrap();
        stream
            .write_all(if framing.contains("chunked") {
                b"1\r\n{\r\n"
            } else {
                b"{"
            })
            .unwrap();
        // Hold the connection open until the client gives up and hangs up.
        while stream.read(&mut request).unwrap_or(0) > 0 {}
    });
    let options = ClientOptions {
        timeout: Duration::from_millis(1500),
        ..ClientOptions::default()
    };
    let client = Client::new("k", BaseUrl::parse(&url).unwrap(), options);
    let started = std::time::Instant::now();
    let error = client.get("/v1/workouts", &[]).unwrap_err();
    assert_eq!(error.code, Code::Timeout);
    assert!(
        started.elapsed() < Duration::from_millis(2800),
        "a timeout is final, not retried"
    );
    server.join().unwrap();
}
