//! HTTP response bounds and content-coding regressions for freshness probes.

use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::p12_harness as harness;

const RESPONSE_CAP: usize = 512 * 1024;

fn gzip_bytes(input: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut child = Command::new("python3")
        .arg("-c")
        .arg("import gzip,sys;sys.stdout.buffer.write(gzip.compress(sys.stdin.buffer.read(),mtime=0))")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("gzip stdin unavailable"))?;
    stdin.write_all(input)?;
    drop(stdin);
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(std::io::Error::other("gzip fixture generation failed").into());
    }
    Ok(output.stdout)
}

fn rewrite_probe_sources(fixture: &harness::Fixture, base_url: &str) -> Result<(), Box<dyn Error>> {
    let path = fixture.dir.join(".velnor/freshness-inventory.json");
    let source = std::fs::read_to_string(&path)?;
    let mut inventory: serde_json::Value = serde_json::from_str(&source)?;
    for (section, label) in [("tools", "tool"), ("actions", "action")] {
        let rows = inventory
            .get_mut(section)
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| std::io::Error::other(format!("missing {section} rows")))?;
        for (index, row) in rows.iter_mut().enumerate() {
            let fields = row
                .as_object_mut()
                .ok_or_else(|| std::io::Error::other("probe row is not an object"))?;
            if !fields.contains_key("source") {
                return Err(std::io::Error::other("probe row has no source").into());
            }
            fields.insert(
                "source".to_owned(),
                serde_json::Value::String(format!("{base_url}/{label}-{index}.json")),
            );
        }
    }
    std::fs::write(path, serde_json::to_vec(&inventory)?)?;
    Ok(())
}

fn respond_to_probe(
    stream: &mut TcpStream,
    encodings: &[String],
    body: &[u8],
) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut request = [0_u8; 4096];
    if stream.read(&mut request)? == 0 {
        return Err(std::io::Error::other("empty HTTP request"));
    }
    let mut coding = String::new();
    for value in encodings {
        coding.push_str("Content-Encoding: ");
        coding.push_str(value);
        coding.push_str("\r\n");
    }
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{coding}Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes())?;
    match stream.write_all(body) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionReset
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn run_http_probe(
    prefix: &str,
    encodings: &[&str],
    body: &[u8],
) -> Result<harness::Run, Box<dyn Error>> {
    let fixture = harness::passing(prefix)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let base_url = format!("http://{}", listener.local_addr()?);
    rewrite_probe_sources(&fixture, &base_url)?;

    let (stop_tx, stop_rx) = mpsc::channel();
    let response_body = body.to_vec();
    let content_encodings = encodings
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    let server = thread::spawn(move || -> std::io::Result<usize> {
        let mut requests = 0;
        loop {
            match stop_rx.try_recv() {
                Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {}
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    requests += 1;
                    respond_to_probe(&mut stream, &content_encodings, &response_body)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(requests)
    });

    let run_result = harness::run_script(&fixture.dir, &["--check-upstream"]);
    drop(stop_tx);
    let server_result = server
        .join()
        .map_err(|_| std::io::Error::other("HTTP fixture server panicked"));
    harness::cleanup(&fixture);
    let run = run_result?;
    let requests = server_result??;
    if requests == 0 {
        return Err(std::io::Error::other(format!(
            "freshness probe {prefix} made no HTTP requests (fixture={}, url={base_url}, exit={}, stdout={:?}, stderr={:?})",
            fixture.dir.display(),
            run.code,
            run.stdout,
            run.stderr
        ))
        .into());
    }
    Ok(run)
}

fn mise_probe_passed(run: &harness::Run) -> bool {
    run.stdout.lines().any(|line| {
        line.starts_with("row: ")
            && line.contains("\"check\":\"upstream-probe\"")
            && line.contains("\"status\":\"pass\"")
            && line.contains("\"subject\":\"mise\"")
    })
}

fn assert_mise_pass(run: &harness::Run) {
    assert!(
        mise_probe_passed(run),
        "mise probe did not parse latest version:\n{}",
        run.stdout
    );
}

#[test]
fn identity_http_response_is_parsed() -> Result<(), Box<dyn Error>> {
    for (prefix, encoding) in [
        ("p12-http-identity", &[][..]),
        ("p12-http-identity-token", &["identity"]),
    ] {
        let run = run_http_probe(prefix, encoding, br#"{"tag_name":"v2026.10.4"}"#)?;
        assert_mise_pass(&run);
    }
    Ok(())
}

#[test]
fn gzip_http_response_is_parsed() -> Result<(), Box<dyn Error>> {
    let body = gzip_bytes(br#"{"tag_name":"v2026.10.4"}"#)?;
    let run = run_http_probe("p12-http-gzip", &["gzip"], &body)?;
    assert_mise_pass(&run);
    Ok(())
}

#[test]
fn unsupported_content_encoding_fails_closed() -> Result<(), Box<dyn Error>> {
    let run = run_http_probe(
        "p12-http-unsupported",
        &["br"],
        br#"{"tag_name":"v2026.10.4"}"#,
    )?;
    harness::assert_fail(&run, "unsupported Content-Encoding");
    assert!(!mise_probe_passed(&run));
    Ok(())
}

#[test]
fn stacked_and_duplicate_content_encodings_fail_closed() -> Result<(), Box<dyn Error>> {
    for (prefix, encodings) in [
        ("p12-http-stacked-encoding", &["gzip, identity"][..]),
        ("p12-http-duplicate-encoding", &["gzip", "gzip"][..]),
    ] {
        let run = run_http_probe(prefix, encodings, br#"{"tag_name":"v2026.10.4"}"#)?;
        harness::assert_fail(&run, "unsupported Content-Encoding");
        assert!(!mise_probe_passed(&run));
    }
    Ok(())
}

#[test]
fn malformed_and_truncated_gzip_fail_closed() -> Result<(), Box<dyn Error>> {
    let mut truncated = gzip_bytes(br#"{"tag_name":"v2026.10.4"}"#)?;
    truncated.truncate(truncated.len() - 4);
    for (prefix, body) in [
        ("p12-http-malformed", b"not gzip".to_vec()),
        ("p12-http-truncated", truncated),
    ] {
        let run = run_http_probe(prefix, &["gzip"], &body)?;
        harness::assert_fail(&run, "lookup_failed");
        assert!(run.stdout.contains("lookup_failed ("), "{}", run.stdout);
        assert!(!mise_probe_passed(&run));
    }
    Ok(())
}

#[test]
fn decompressed_response_cap_fails_closed() -> Result<(), Box<dyn Error>> {
    let expanded = vec![b'a'; RESPONSE_CAP + 1];
    let body = gzip_bytes(&expanded)?;
    let run = run_http_probe("p12-http-decoded-limit", &["gzip"], &body)?;
    harness::assert_fail(&run, "decompressed response exceeds");
    Ok(())
}

#[test]
fn encoded_response_cap_fails_closed() -> Result<(), Box<dyn Error>> {
    let body = vec![b'a'; RESPONSE_CAP + 1];
    let run = run_http_probe("p12-http-encoded-limit", &["identity"], &body)?;
    harness::assert_fail(&run, "encoded response exceeds");
    Ok(())
}

#[test]
fn gzip_encoded_response_cap_fails_closed() -> Result<(), Box<dyn Error>> {
    let mut state = 0x9e37_79b9_u32;
    let expanded = (0..RESPONSE_CAP)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect::<Vec<_>>();
    let body = gzip_bytes(&expanded)?;
    assert_eq!(expanded.len(), RESPONSE_CAP);
    assert!(
        body.len() > RESPONSE_CAP,
        "gzip fixture must exceed the encoded response cap"
    );
    let run = run_http_probe("p12-http-gzip-encoded-limit", &["gzip"], &body)?;
    harness::assert_fail(&run, "encoded response exceeds");
    Ok(())
}
