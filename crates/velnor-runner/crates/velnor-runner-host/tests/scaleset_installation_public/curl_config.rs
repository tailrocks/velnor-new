// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::TestResult;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
use velnor_runner_github::{Method, SessionRequest};
use velnor_runner_host::curl_config::encode;

fn request(method: Method) -> SessionRequest {
    SessionRequest {
        method,
        path: "/".to_owned(),
        query: None,
        headers: vec![("X-Trace".to_owned(), r"literal\nheader".to_owned())],
        body: Vec::new(),
    }
}

fn curl(config: &str, cwd: &Path) -> TestResult<std::process::Output> {
    let mut child = Command::new("/usr/bin/curl")
        .args(["--disable", "--noproxy", "*", "--config", "-"])
        .current_dir(cwd)
        .env_remove("http_proxy")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("missing curl stdin")?
        .write_all(config.as_bytes())?;
    Ok(child.wait_with_output()?)
}

#[test]
fn real_curl_reads_literal_backslash_in_file_url() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    for suffix in [r"a\xb", r"a\nb"] {
        fs::write(dir.path().join(suffix), b"literal")?;
        fs::write(dir.path().join(suffix.replace('\\', "")), b"decoy")?;
        let url = format!("file://{}", dir.path().join(suffix).display());
        let config = encode(
            &url,
            &request(Method::Get),
            Path::new("unused"),
            Duration::from_secs(3),
        )?;
        let output = curl(config.as_str(), dir.path())?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.starts_with(b"literal"));
    }
    Ok(())
}

#[test]
fn real_curl_preserves_header_and_relative_body_path() -> TestResult<()> {
    let dir = tempfile::tempdir()?;
    for (name, method) in [
        (r"body\nfile", Method::Post),
        (r"body\xfile", Method::Patch),
    ] {
        fs::write(dir.path().join(name), b"literal-body")?;
        fs::write(dir.path().join(name.replace('\\', "")), b"decoy")?;
        let (address, peer) = spawn_peer()?;
        let config = encode(
            &format!("http://{address}/"),
            &request(method),
            Path::new(name),
            Duration::from_secs(3),
        )?;
        let output = curl(config.as_str(), dir.path())?;
        let peer_result = peer.join().map_err(|_| "peer panicked")?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bytes = peer_result?;
        assert!(
            bytes
                .windows(b"X-Trace: literal\\nheader\r\n".len())
                .any(|part| part == b"X-Trace: literal\\nheader\r\n")
        );
        assert!(bytes.ends_with(b"literal-body"));
    }
    Ok(())
}

fn spawn_peer() -> TestResult<(
    std::net::SocketAddr,
    std::thread::JoinHandle<std::io::Result<Vec<u8>>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let peer = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(e),
            }
        };
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 1024];
        while !bytes
            .windows(b"literal-body".len())
            .any(|part| part == b"literal-body")
        {
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if bytes.len() > 16384 {
                return Err(std::io::Error::other("request too large"));
            }
        }
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")?;
        Ok(bytes)
    });
    Ok((address, peer))
}

#[test]
fn codec_rejects_controls_and_stdin_body_without_io() {
    use std::os::unix::ffi::OsStrExt;
    let invalid_path = Path::new(std::ffi::OsStr::from_bytes(b"bad\xff"));
    assert!(
        encode(
            "file:///unused",
            &request(Method::Post),
            invalid_path,
            Duration::from_secs(1)
        )
        .is_err()
    );
    assert!(
        encode(
            "file:///unused",
            &request(Method::Post),
            Path::new("./-"),
            Duration::from_secs(1)
        )
        .is_ok()
    );
    for value in ["bad\nvalue", "bad\rvalue", "bad\0value", "bad\"value"] {
        assert!(
            encode(
                value,
                &request(Method::Get),
                Path::new("unused"),
                Duration::from_secs(1)
            )
            .is_err()
        );
    }
    assert!(
        encode(
            "file:///unused",
            &request(Method::Post),
            Path::new("-"),
            Duration::from_secs(1)
        )
        .is_err()
    );
}

#[test]
fn codec_keeps_positive_curl_timeout_nonzero() -> TestResult<()> {
    let config = encode(
        "file:///unused",
        &request(Method::Get),
        Path::new("unused"),
        Duration::from_secs(1),
    )?;
    assert_eq!(format!("{config:?}"), "CurlConfig([REDACTED])");
    let cases = [
        (Duration::from_nanos(1), "0.001"),
        (Duration::from_micros(999), "0.001"),
        (Duration::from_millis(1), "0.001"),
        (Duration::from_millis(1_250), "1.250"),
        (Duration::from_micros(1_250_001), "1.251"),
    ];
    for (duration, expected) in cases {
        let config = encode(
            "file:///unused",
            &request(Method::Get),
            Path::new("unused"),
            duration,
        )?;
        assert!(config.as_str().contains(&format!("max-time = {expected}")));
    }
    assert!(
        encode(
            "file:///unused",
            &request(Method::Get),
            Path::new("unused"),
            Duration::ZERO
        )
        .is_err()
    );
    Ok(())
}
