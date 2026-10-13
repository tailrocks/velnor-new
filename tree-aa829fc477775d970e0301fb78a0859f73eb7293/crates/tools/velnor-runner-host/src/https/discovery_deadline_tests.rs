use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use velnor_runner_github::{
    BearerRole, DiscoveryTransport, Method, RequestPurpose, SessionRequest, TransportFail,
};

use super::BoundedDiscoveryTransport;

fn github_get() -> SessionRequest {
    SessionRequest {
        purpose: RequestPurpose::RepositoryRead,
        bearer_role: BearerRole::GithubRestCredential,
        method: Method::Get,
        path: "repos/acme/widget".to_owned(),
        query: None,
        headers: vec![
            (
                "Accept".to_owned(),
                "application/vnd.github+json".to_owned(),
            ),
            ("Authorization".to_owned(), "Bearer test-token".to_owned()),
            ("X-GitHub-Api-Version".to_owned(), "2026-03-10".to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
        ],
        body: Vec::new(),
    }
}

fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    listener
        .set_nonblocking(true)
        .expect("listener should be nonblocking");
    let base = format!(
        "http://{}",
        listener
            .local_addr()
            .expect("listener address should resolve")
    );
    (listener, base)
}

fn expired_cutoff() -> Instant {
    Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("monotonic clock should have a prior instant")
}

fn slow_server(
    response_delay: Duration,
    accepted: Option<mpsc::Sender<()>>,
) -> (String, thread::JoinHandle<bool>) {
    let (listener, base) = listener();
    let worker = thread::spawn(move || {
        let accept_deadline = Instant::now() + Duration::from_secs(2);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(pair) => break pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= accept_deadline {
                        return false;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return false,
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .expect("server read timeout should be set");
        let _request = read_headers(&mut stream);
        if let Some(accepted) = accepted {
            accepted
                .send(())
                .expect("request acceptance should reach its observer");
        }
        thread::sleep(response_delay);
        match stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
        {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => true,
            Err(_) => false,
        }
    });
    (base, worker)
}

fn read_headers(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 1024];
    while bytes.len() < 16 * 1024 {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    bytes
}

fn assert_no_connection(listener: &TcpListener) {
    let deadline = Instant::now() + Duration::from_millis(120);
    loop {
        match listener.accept() {
            Ok((_stream, _)) => panic!("expired/cancelled exchange reached the wire"),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("listener accept failed: {error}"),
        }
    }
}

#[test]
fn expired_and_cancelled_sync_and_async_calls_do_not_dispatch() {
    let (listener, base) = listener();
    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(20));
    transport
        .bind_github_api_origin()
        .expect("GitHub origin should bind");

    let cancelled = AtomicBool::new(true);
    assert_eq!(
        transport.exchange_until(&github_get(), None, &cancelled),
        Err(TransportFail::Reset)
    );

    let cutoff = expired_cutoff();
    assert_eq!(
        transport.exchange_until(&github_get(), Some(cutoff), &AtomicBool::new(false)),
        Err(TransportFail::Timeout)
    );

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    let cancellation = Arc::new(AtomicBool::new(false));
    let exchange =
        transport.exchange_discovery_until(github_get(), Some(expired_cutoff()), cancellation);
    assert_eq!(runtime.block_on(exchange), Err(TransportFail::Timeout));
    assert_no_connection(&listener);
}

#[test]
fn sync_absolute_cutoff_wins_over_the_per_request_deadline() {
    let (base, server) = slow_server(Duration::from_millis(700), None);
    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(20));
    transport
        .bind_github_api_origin()
        .expect("GitHub origin should bind");
    let started = Instant::now();
    let result = transport.exchange_until(
        &github_get(),
        Some(started + Duration::from_millis(180)),
        &AtomicBool::new(false),
    );
    assert_eq!(result, Err(TransportFail::Timeout));
    assert!(started.elapsed() < Duration::from_millis(550));
    assert!(server.join().expect("server should finish"));
}

#[test]
fn sync_external_cancellation_stops_an_already_dispatched_request() {
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (base, server) = slow_server(Duration::from_millis(700), Some(accepted_tx));
    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(20));
    transport
        .bind_github_api_origin()
        .expect("GitHub origin should bind");
    let cancellation = Arc::new(AtomicBool::new(false));
    let cancel_after_dispatch = Arc::clone(&cancellation);
    let canceller = thread::spawn(move || {
        if accepted_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
            cancel_after_dispatch.store(true, Ordering::Release);
        }
    });
    let result = transport.exchange_until(
        &github_get(),
        Some(Instant::now() + Duration::from_secs(2)),
        &cancellation,
    );
    assert_eq!(result, Err(TransportFail::Reset));
    canceller.join().expect("cancellation thread should finish");
    assert!(server.join().expect("server should finish"));
}

#[test]
fn async_absolute_cutoff_wins_over_the_per_request_deadline() {
    let (base, server) = slow_server(Duration::from_millis(700), None);
    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(20));
    transport
        .bind_github_api_origin()
        .expect("GitHub origin should bind");
    let started = Instant::now();
    let exchange = transport.exchange_discovery_until(
        github_get(),
        Some(started + Duration::from_millis(180)),
        Arc::new(AtomicBool::new(false)),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    assert_eq!(runtime.block_on(exchange), Err(TransportFail::Timeout));
    assert!(started.elapsed() < Duration::from_millis(550));
    assert!(server.join().expect("server should finish"));
}

#[test]
fn async_external_cancellation_stops_an_already_dispatched_request() {
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (base, server) = slow_server(Duration::from_millis(700), Some(accepted_tx));
    let mut transport = BoundedDiscoveryTransport::for_test(base, 4096, Duration::from_secs(20));
    transport
        .bind_github_api_origin()
        .expect("GitHub origin should bind");
    let cancellation = Arc::new(AtomicBool::new(false));
    let cancel_after_dispatch = Arc::clone(&cancellation);
    let canceller = thread::spawn(move || {
        if accepted_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
            cancel_after_dispatch.store(true, Ordering::Release);
        }
    });
    let exchange = transport.exchange_discovery_until(
        github_get(),
        Some(Instant::now() + Duration::from_secs(2)),
        cancellation,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    assert_eq!(runtime.block_on(exchange), Err(TransportFail::Reset));
    canceller.join().expect("cancellation thread should finish");
    assert!(server.join().expect("server should finish"));
}
