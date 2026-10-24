use std::{
    fs,
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use tempfile::tempdir;

use crate::{certificates, client};

const TEST_NONCE: &str = "9b8a7c6d5e4f32109b8a7c6d5e4f32109b8a7c6d5e4f32109b8a7c6d5e4f3210";
const SERVER_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REQUEST_BYTES: usize = 8_192;

#[tokio::test]
async fn localhost_https_accepts_only_private_ca_and_dns_name() -> Result<()> {
    let directory = tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    certificates::generate_bundle(directory.path())?;
    let bundle = certificates::validate_bundle(directory.path())?;
    let server = LocalTlsServer::start(&bundle.server_certificate, &bundle.server_key, TEST_NONCE)?;
    let ca = fs::read(&bundle.ca)?;
    let trusted = client::client(&ca, None)?;

    let response = client::get_text(&trusted, "localhost", server.port(), "/healthz").await?;
    ensure!(
        response == (200, "ok".to_owned()),
        "localhost HTTPS health check failed"
    );
    let response = client::get_text(&trusted, "localhost", server.port(), "/nonce").await?;
    ensure!(
        response == (200, TEST_NONCE.to_owned()),
        "localhost HTTPS nonce did not match"
    );

    let untrusted = client::no_trusted_roots_client()?;
    let result = client::request(&untrusted, "localhost", server.port(), "/nonce").await;
    client::require_certificate_rejection(
        result,
        client::CertificateRejection::UnknownIssuer,
        "untrusted CA",
    )?;

    let wrong_name = client::client(&ca, Some(("wrong.invalid", server.address())))?;
    let result = client::request(&wrong_name, "wrong.invalid", server.port(), "/nonce").await;
    client::require_certificate_rejection(
        result,
        client::CertificateRejection::NameMismatch,
        "wrong DNS identity",
    )?;

    let result = client::request(&trusted, "127.0.0.1", server.port(), "/nonce").await;
    client::require_certificate_rejection(
        result,
        client::CertificateRejection::NameMismatch,
        "IP identity against DNS-only SAN",
    )?;

    let response = client::get_text(&trusted, "localhost", server.port(), "/nonce").await?;
    ensure!(
        response == (200, TEST_NONCE.to_owned()),
        "server did not remain healthy after TLS rejections"
    );
    Ok(())
}

struct LocalTlsServer {
    port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LocalTlsServer {
    fn start(
        certificate_path: &std::path::Path,
        key_path: &std::path::Path,
        nonce: &str,
    ) -> Result<Self> {
        let certificate = CertificateDer::from_pem_file(certificate_path)
            .context("could not parse generated server certificate")?;
        let private_key = PrivateKeyDer::from_pem_file(key_path)
            .context("could not parse generated server key")?;
        ensure!(
            fs::metadata(key_path)?.permissions().mode() & 0o777 == 0o600,
            "local TLS server key must be mode 0600"
        );
        let config = Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![certificate], private_key)
                .context("could not configure local TLS server")?,
        );
        let (listeners, port) = localhost_listeners()?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let nonce = nonce.to_owned();
        let worker = thread::Builder::new()
            .name("fixture-local-tls".to_owned())
            .spawn(move || serve_listeners(&listeners, &config, &thread_stop, &nonce))?;
        Ok(Self {
            port,
            stop,
            worker: Some(worker),
        })
    }

    fn address(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }

    const fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for LocalTlsServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn localhost_listeners() -> Result<(Vec<TcpListener>, u16)> {
    let ipv4 =
        TcpListener::bind(("127.0.0.1", 0)).context("could not bind local IPv4 test listener")?;
    let port = ipv4.local_addr()?.port();
    ipv4.set_nonblocking(true)?;
    let mut listeners = vec![ipv4];
    if let Ok(ipv6) = TcpListener::bind(("::1", port)) {
        ipv6.set_nonblocking(true)?;
        listeners.push(ipv6);
    }
    Ok((listeners, port))
}

fn serve_listeners(
    listeners: &[TcpListener],
    config: &Arc<ServerConfig>,
    stop: &Arc<AtomicBool>,
    nonce: &str,
) {
    while !stop.load(Ordering::Acquire) {
        let mut accepted = false;
        for listener in listeners {
            match listener.accept() {
                Ok((stream, _)) => {
                    accepted = true;
                    if let Err(error) = handle_connection(stream, Arc::clone(config), nonce) {
                        eprintln!("local HTTPS fixture connection failed: {error:#}");
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => return,
            }
        }
        if !accepted {
            thread::sleep(Duration::from_millis(5));
        }
    }
}

fn handle_connection(stream: TcpStream, config: Arc<ServerConfig>, nonce: &str) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(SERVER_READ_TIMEOUT))?;
    stream.set_write_timeout(Some(SERVER_READ_TIMEOUT))?;
    let connection =
        ServerConnection::new(config).context("could not start TLS server connection")?;
    let mut tls = StreamOwned::new(connection, stream);
    let request = read_request(&mut tls)?;
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .unwrap_or_default();
    let body = match path {
        "/healthz" => "ok",
        "/nonce" => nonce,
        _ => "not found",
    };
    let status = if path == "/healthz" || path == "/nonce" {
        "200 OK"
    } else {
        "404 Not Found"
    };
    write!(
        tls,
        "HTTP/1.1 {status}\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )?;
    tls.flush()?;
    Ok(())
}

fn read_request(stream: &mut impl Read) -> Result<String> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1_024];
    while request.len() < MAX_REQUEST_BYTES {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    ensure!(
        request.len() < MAX_REQUEST_BYTES,
        "local HTTPS request exceeded its bound"
    );
    String::from_utf8(request).context("local HTTPS request was not UTF-8")
}
