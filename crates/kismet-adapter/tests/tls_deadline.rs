use kyberia_kismet_adapter::live::{
    ApiToken, CancellationToken, Endpoint, KismetLiveClient, LiveLimits,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn incomplete_tls_record_cannot_extend_the_poll_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = Endpoint::new(format!("https://{}", listener.local_addr().unwrap())).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut hello = [0_u8; 8192];
        let _ = stream.read(&mut hello).unwrap();
        // An incomplete TLS record whose fragments arrive before any
        // inactivity timeout. A request deadline must still bound the whole
        // handshake, rather than only each socket read.
        stream.write_all(&[0x16, 0x03, 0x03, 0x00, 0x10]).unwrap();
        for _ in 0..16 {
            thread::sleep(Duration::from_millis(40));
            if stream.write_all(&[0]).is_err() {
                break;
            }
        }
    });
    let client = KismetLiveClient::connect(
        endpoint,
        ApiToken::new("test-only-token").unwrap(),
        LiveLimits {
            request_timeout: Duration::from_millis(100),
            max_retries: 0,
            ..LiveLimits::default()
        },
    )
    .unwrap();
    let started = Instant::now();
    let result = client.poll(&CancellationToken::default());
    let elapsed = started.elapsed();
    server.join().unwrap();
    assert!(result.is_err());
    assert!(
        elapsed < Duration::from_millis(350),
        "100ms deadline took {elapsed:?}: {result:?}"
    );
}

#[test]
fn explicit_address_keeps_hostname_for_tls_sni() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = Endpoint::new(format!("https://kismet.example.invalid:{}", address.port()))
        .unwrap()
        .with_resolved_addresses([address])
        .unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut hello = [0_u8; 8192];
        let count = stream.read(&mut hello).unwrap();
        assert!(
            hello[..count]
                .windows(b"kismet.example.invalid".len())
                .any(|window| window == b"kismet.example.invalid"),
            "TLS ClientHello did not retain the URL hostname"
        );
    });
    let client = KismetLiveClient::connect(
        endpoint,
        ApiToken::new("test-only-token").unwrap(),
        LiveLimits {
            request_timeout: Duration::from_millis(200),
            max_retries: 0,
            ..LiveLimits::default()
        },
    )
    .unwrap();
    assert!(client.poll(&CancellationToken::default()).is_err());
    server.join().unwrap();
}
