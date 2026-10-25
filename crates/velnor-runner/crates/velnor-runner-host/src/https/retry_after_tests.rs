use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, UNIX_EPOCH};

use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use super::{OriginKey, origin_key, parse_retry_after, record_at, remaining_at};
use crate::https::HttpsTransport;

const ORIGIN_COUNT: usize = 300;

#[test]
fn retry_after_parses_seconds_dates_and_invalid_values() {
    let now = UNIX_EPOCH + Duration::from_hours(500_000);
    let future_date = httpdate::fmt_http_date(now + Duration::from_secs(30));
    let past_date = httpdate::fmt_http_date(now - Duration::from_secs(30));
    let maximum_seconds = u64::MAX.to_string();
    let cases = [
        (b"0".as_slice(), Some(Duration::ZERO)),
        (b"17", Some(Duration::from_secs(17))),
        (b" 17 \r\n", Some(Duration::from_secs(17))),
        (
            maximum_seconds.as_bytes(),
            Some(Duration::from_secs(u64::MAX)),
        ),
        (b"18446744073709551616", None),
        (b"-1", None),
        (b"+1", None),
        (b"", None),
        (b"tomorrow", None),
    ];

    for (value, expected) in cases {
        assert_eq!(parse_retry_after(value, now), expected);
    }
    assert_eq!(
        parse_retry_after(future_date.as_bytes(), now),
        Some(Duration::from_secs(30))
    );
    assert_eq!(
        parse_retry_after(past_date.as_bytes(), now),
        Some(Duration::ZERO)
    );
    assert_eq!(parse_retry_after(&[0xff], now), None);
}

#[test]
fn origin_keys_normalize_paths_hosts_and_default_ports() {
    let first = origin_key("https://API.GITHUB.COM/v3").expect("HTTPS origin");
    let same = origin_key("https://api.github.com:443/repos").expect("same origin");
    let other_port = origin_key("https://api.github.com:444/repos").expect("other port");
    let other_scheme = origin_key("http://api.github.com/repos").expect("other scheme");

    assert_eq!(first, same);
    assert_ne!(first, other_port);
    assert_ne!(first, other_scheme);
}

#[test]
fn holds_extend_but_never_shorten_and_expired_entries_are_removed() {
    let key = OriginKey("https://api.example".to_owned());
    let now = Instant::now();
    let mut registry = BTreeMap::new();
    record_at(&mut registry, key.clone(), Duration::from_secs(10), now);
    record_at(
        &mut registry,
        key.clone(),
        Duration::from_secs(5),
        now + Duration::from_secs(2),
    );
    assert_eq!(
        remaining_at(&mut registry, &key, now + Duration::from_secs(2)),
        Duration::from_secs(8)
    );

    record_at(
        &mut registry,
        key.clone(),
        Duration::from_secs(20),
        now + Duration::from_secs(3),
    );
    assert_eq!(
        remaining_at(&mut registry, &key, now + Duration::from_secs(3)),
        Duration::from_secs(20)
    );
    assert_eq!(
        remaining_at(&mut registry, &key, now + Duration::from_secs(24)),
        Duration::ZERO
    );
    assert!(registry.is_empty());
}

#[test]
fn stale_publication_cannot_shorten_a_later_observed_hold() {
    let earlier = Instant::now();
    let later = earlier + Duration::from_secs(2);
    let key = OriginKey("https://api.example".to_owned());
    let mut registry = BTreeMap::new();

    record_at(&mut registry, key.clone(), Duration::from_secs(9), later);
    record_at(&mut registry, key.clone(), Duration::from_secs(10), earlier);

    assert_eq!(
        remaining_at(&mut registry, &key, later),
        Duration::from_secs(9)
    );
    assert_eq!(
        remaining_at(&mut registry, &key, later + Duration::from_secs(9)),
        Duration::ZERO
    );
}

#[test]
fn holds_are_isolated_by_origin_and_more_than_256_active_origins_are_retained() {
    let now = Instant::now();
    let first = OriginKey("https://first.example".to_owned());
    let second = OriginKey("https://second.example".to_owned());
    let mut registry = BTreeMap::new();
    record_at(&mut registry, first.clone(), Duration::from_secs(10), now);
    record_at(&mut registry, second.clone(), Duration::from_secs(20), now);

    assert_eq!(
        remaining_at(&mut registry, &first, now),
        Duration::from_secs(10)
    );
    assert_eq!(
        remaining_at(&mut registry, &second, now),
        Duration::from_secs(20)
    );

    let mut many_origins = BTreeMap::new();
    for index in 0..ORIGIN_COUNT {
        record_at(
            &mut many_origins,
            OriginKey(format!("https://{index}.example")),
            Duration::from_secs(30),
            now,
        );
    }
    assert_eq!(many_origins.len(), ORIGIN_COUNT);
    for index in 0..ORIGIN_COUNT {
        let key = OriginKey(format!("https://{index}.example"));
        assert_eq!(
            remaining_at(&mut many_origins, &key, now),
            Duration::from_secs(30)
        );
    }

    let first_many = OriginKey("https://0.example".to_owned());
    record_at(
        &mut many_origins,
        first_many.clone(),
        Duration::from_secs(60),
        now,
    );
    assert_eq!(
        remaining_at(&mut many_origins, &first_many, now),
        Duration::from_secs(60)
    );
}

#[test]
fn successful_429_and_503_return_the_current_body_after_one_curl_request()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, body) in [(429, b"throttled".as_slice()), (503, b"unavailable")] {
        let fixture = RetryAfterFixture::start(vec![response(status, Some("1"), body.to_vec())])?;
        let mut transport = fixture_transport(&fixture.base);
        let exchange = exchange(&mut transport)?;
        let count = fixture.finish()?;

        assert_eq!(exchange.status, status);
        assert_eq!(exchange.body, body);
        assert_eq!(count, 1);
    }
    Ok(())
}

#[test]
fn clone_bounded_and_recreated_transports_keep_the_origin_hold()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = RetryAfterFixture::start(vec![
        response(429, Some("1"), b"first".to_vec()),
        response(503, Some("1"), b"second".to_vec()),
        response(200, None, b"ready".to_vec()),
    ])?;
    let mut original = fixture_transport(&fixture.base);
    let first = exchange(&mut original)?;
    assert_eq!(first.status, 429);
    assert_eq!(first.body.as_slice(), b"first".as_slice());

    let mut bounded_clone = original
        .clone()
        .bounded_by_deadline(Instant::now() + Duration::from_secs(4));
    let second_started = Instant::now();
    let second = exchange(&mut bounded_clone)?;
    assert!(second_started.elapsed() >= Duration::from_millis(800));
    assert_eq!(second.status, 503);
    assert_eq!(second.body.as_slice(), b"second".as_slice());

    let mut recreated = fixture_transport(&fixture.base);
    let third_started = Instant::now();
    let third = exchange(&mut recreated)?;
    assert!(third_started.elapsed() >= Duration::from_millis(800));
    assert_eq!(third.status, 200);
    assert_eq!(third.body.as_slice(), b"ready".as_slice());
    assert_eq!(fixture.finish()?, 3);
    Ok(())
}

#[test]
fn deadline_during_a_hold_expires_without_a_second_curl_request()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture =
        RetryAfterFixture::start(vec![response(429, Some("2"), b"current response".to_vec())])?;
    let mut first_transport = fixture_transport(&fixture.base);
    let first = exchange(&mut first_transport)?;
    assert_eq!(first.status, 429);
    assert_eq!(first.body.as_slice(), b"current response".as_slice());

    let deadline = Instant::now() + Duration::from_millis(120);
    let mut bounded = fixture_transport(&fixture.base).bounded_by_deadline(deadline);
    let started = Instant::now();
    assert!(matches!(
        bounded.exchange(&request()),
        Err(TransportFail::Timeout)
    ));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(fixture.finish()?, 1);
    Ok(())
}

#[test]
fn oversized_retry_after_is_drained_without_changing_response_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = RetryAfterFixture::start(vec![
        response(429, Some(&"x".repeat(300)), b"preserved".to_vec()),
        response(200, None, b"next".to_vec()),
    ])?;
    let mut transport = fixture_transport(&fixture.base);
    let first = exchange(&mut transport)?;
    assert_eq!(first.status, 429);
    assert_eq!(first.body.as_slice(), b"preserved".as_slice());

    let started = Instant::now();
    let second = exchange(&mut transport)?;
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(second.status, 200);
    assert_eq!(second.body.as_slice(), b"next".as_slice());
    assert_eq!(fixture.finish()?, 2);
    Ok(())
}

fn fixture_transport(base: &str) -> HttpsTransport {
    HttpsTransport {
        base: base.to_owned(),
        max_time_seconds: 5,
        deadline: None,
    }
}

fn request() -> SessionRequest {
    SessionRequest {
        method: Method::Get,
        path: "/resource".to_owned(),
        query: None,
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn exchange(transport: &mut HttpsTransport) -> io::Result<Exchange> {
    transport
        .exchange(&request())
        .map_err(|error| io::Error::other(format!("HTTPS exchange failed: {error:?}")))
}

fn response(status: u16, retry_after: Option<&str>, body: Vec<u8>) -> FixtureResponse {
    FixtureResponse {
        status,
        retry_after: retry_after.map(str::to_owned),
        body,
    }
}

struct FixtureResponse {
    status: u16,
    retry_after: Option<String>,
    body: Vec<u8>,
}

struct RetryAfterFixture {
    base: String,
    requests: JoinHandle<io::Result<usize>>,
}

impl RetryAfterFixture {
    fn start(responses: Vec<FixtureResponse>) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let requests = thread::spawn(move || serve(&listener, responses));
        Ok(Self {
            base: format!("http://{address}"),
            requests,
        })
    }

    fn finish(self) -> io::Result<usize> {
        self.requests
            .join()
            .map_err(|_| io::Error::other("Retry-After fixture thread panicked"))?
    }
}

fn serve(listener: &TcpListener, responses: Vec<FixtureResponse>) -> io::Result<usize> {
    let mut count = 0;
    for response in responses {
        let (mut stream, _) = accept_before(listener, Instant::now() + Duration::from_secs(5))?;
        read_headers(&mut stream)?;
        write_response(&mut stream, response)?;
        count += 1;
    }
    Ok(count)
}

fn accept_before(
    listener: &TcpListener,
    deadline: Instant,
) -> io::Result<(TcpStream, std::net::SocketAddr)> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fixture accept timed out",
            ));
        }
        match listener.accept() {
            Ok(connection) => return Ok(connection),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5).min(remaining));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

fn read_headers(stream: &mut TcpStream) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended before its headers",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > 16 * 1024 {
            return Err(io::Error::other("request headers exceeded fixture bound"));
        }
    }
    Ok(())
}

fn write_response(stream: &mut TcpStream, response: FixtureResponse) -> io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Fixture Response",
    };
    let mut headers = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.body.len()
    );
    if let Some(retry_after) = response.retry_after {
        headers.push_str("Retry-After: ");
        headers.push_str(&retry_after);
        headers.push_str("\r\n");
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes())?;
    stream.write_all(&response.body)
}
