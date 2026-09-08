use kyberia_kismet_adapter::live::{
    ApiToken, CancellationToken, Endpoint, KismetLiveClient, LiveError, LiveLimits,
};
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;

const TOKEN: &str = "local-fixture-secret";
const STATUS: &str = r#"{"kismet.system.version":"2026.09.0-fixture","kismet.system.git":"2d25ad0","kismet.system.server_name":"local-fixture","kismet.system.devices.count":2}"#;
const TYPES: &str = r#"[{"kismet.datasource.driver.type":"linuxwifi","kismet.datasource.driver.description":"fixture monitor","kismet.datasource.driver.remote_capable":true}]"#;
const SOURCES: &str = r#"[{"kismet.datasource.uuid":"remote-source","kismet.datasource.remote":true,"kismet.datasource.channel":null},{"kismet.datasource.uuid":"local-source","kismet.datasource.remote":false,"kismet.datasource.hopping":true,"kismet.datasource.hop_channels":["1","6"]}]"#;

fn read_request(stream: TcpStream) -> (String, String) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let path = request_line.split_whitespace().nth(1).unwrap().to_owned();
    let mut cookie = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Cookie:") {
            cookie = value.trim().to_owned();
        }
    }
    (path, cookie)
}

#[test]
fn local_http_fixture_uses_cookie_and_only_read_only_paths() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let mut paths = Vec::new();
        for (status, body) in [(200, STATUS), (200, TYPES), (200, SOURCES)] {
            let (mut stream, _) = listener.accept().unwrap();
            let (path, cookie) = read_request(stream.try_clone().unwrap());
            paths.push(path);
            assert_eq!(cookie, format!("KISMET={TOKEN}"));
            let reason = if status == 200 { "OK" } else { "Fixture" };
            write!(
                stream,
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            stream.shutdown(Shutdown::Both).unwrap();
        }
        paths
    });
    let client = KismetLiveClient::connect(
        Endpoint::new(endpoint).unwrap(),
        ApiToken::new(TOKEN).unwrap(),
        LiveLimits::default(),
    )
    .unwrap();
    let snapshot = client.poll(&CancellationToken::default()).unwrap();
    let paths = handle.join().unwrap();
    assert_eq!(
        paths,
        vec![
            "/system/status.json",
            "/datasource/types.json",
            "/datasource/all_sources.json"
        ]
    );
    assert_eq!(snapshot.datasources()[0].uuid(), "local-source");
    assert_eq!(snapshot.datasources()[1].remote(), Some(true));
    assert_eq!(snapshot.datasources()[1].channel(), None);
    assert!(!String::from_utf8_lossy(&snapshot.canonical_bytes()).contains(TOKEN));
}

#[test]
fn unauthorized_response_is_distinct_and_session_check_is_never_called() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (path, cookie) = read_request(stream.try_clone().unwrap());
        assert_eq!(path, "/system/status.json");
        assert_eq!(cookie, format!("KISMET={TOKEN}"));
        let body = "{\"error\":\"token should never appear in client errors\"}";
        write!(
            stream,
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let client = KismetLiveClient::connect(
        Endpoint::new(endpoint).unwrap(),
        ApiToken::new(TOKEN).unwrap(),
        LiveLimits::default(),
    )
    .unwrap();
    assert_eq!(
        client.poll(&CancellationToken::default()),
        Err(LiveError::AuthenticationRequired)
    );
    handle.join().unwrap();
}

#[test]
fn redirects_and_oversized_bodies_fail_without_secret_bearing_errors() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(stream.try_clone().unwrap());
        write!(
            stream,
            "HTTP/1.1 302 Found\r\nLocation: http://redirect.invalid/other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
    });
    let client = KismetLiveClient::connect(
        Endpoint::new(endpoint).unwrap(),
        ApiToken::new(TOKEN).unwrap(),
        LiveLimits::default(),
    )
    .unwrap();
    let error = client.poll(&CancellationToken::default()).unwrap_err();
    assert_eq!(error, LiveError::Redirect);
    assert!(!error.to_string().contains(TOKEN));
    handle.join().unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(stream.try_clone().unwrap());
        let body = "x".repeat(128);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let limits = LiveLimits {
        max_body_bytes: 64,
        ..LiveLimits::default()
    };
    let client = KismetLiveClient::connect(
        Endpoint::new(endpoint).unwrap(),
        ApiToken::new(TOKEN).unwrap(),
        limits,
    )
    .unwrap();
    assert_eq!(
        client.poll(&CancellationToken::default()),
        Err(LiveError::BodyTooLarge)
    );
    handle.join().unwrap();
}

#[test]
fn declared_truncation_is_rejected_before_json_decode() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_request(stream.try_clone().unwrap());
        let body = "{}";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\n{body}"
        )
        .unwrap();
    });
    let client = KismetLiveClient::connect(
        Endpoint::new(endpoint).unwrap(),
        ApiToken::new(TOKEN).unwrap(),
        LiveLimits::default(),
    )
    .unwrap();
    assert_eq!(
        client.poll(&CancellationToken::default()),
        Err(LiveError::TruncatedBody)
    );
    handle.join().unwrap();
}
