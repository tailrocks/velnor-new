use super::{decode_body, fetch_text, sniff_latest};
use crate::context::FETCH_CAP;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write;
use std::net::TcpListener;
use std::thread;

fn response(body: Vec<u8>, encoding: &str) -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let encoding = encoding.to_owned();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let mut request = [0_u8; 1024];
        let _ =
            std::io::Read::read(&mut stream, &mut request).map_err(|error| error.to_string())?;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Encoding: {}\r\nConnection: close\r\n\r\n", body.len(), encoding).map_err(|error| error.to_string())?;
        stream.write_all(&body).map_err(|error| error.to_string())?;
        Ok::<(), String>(())
    });
    let result = fetch_text(&format!("http://{address}/"));
    server
        .join()
        .map_err(|_| "response server panicked".to_owned())??;
    result
}

fn gzip(input: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    let write_result = encoder.write_all(input);
    assert!(write_result.is_ok(), "gzip write: {write_result:?}");
    encoder.finish().unwrap_or_default()
}

#[test]
fn bounded_fetch_accepts_identity_and_gzip() {
    assert_eq!(
        response(b"payload".to_vec(), "identity"),
        Ok("payload".to_owned())
    );
    assert_eq!(response(gzip(b"payload"), "gzip"), Ok("payload".to_owned()));
}

#[test]
fn encoded_and_decoded_caps_are_enforced() {
    let encoded = vec![b'x'; FETCH_CAP + 1];
    assert!(response(encoded, "identity").is_err());
    assert!(response(gzip(&vec![b'x'; FETCH_CAP + 1]), "gzip").is_err());
}

#[test]
fn exact_encoded_and_decoded_caps_are_accepted() {
    let encoded = vec![b'x'; FETCH_CAP];
    assert_eq!(
        response(encoded, "identity").map(|body| body.len()),
        Ok(FETCH_CAP)
    );
    let decoded = vec![b'x'; FETCH_CAP];
    assert_eq!(
        response(gzip(&decoded), "gzip").map(|body| body.len()),
        Ok(FETCH_CAP)
    );
}

#[test]
fn unsupported_and_broken_content_encodings_fail_closed() {
    assert!(decode_body("br", b"body").is_err());
    assert!(decode_body("gzip", b"\x1f\x8b\x08").is_err());
    assert!(response(vec![1, 2, 3], "br").is_err());
}

#[test]
fn release_formats_skip_unstable_github_entries() {
    let source = "https://api.github.com/repos/example/tool/releases/latest";
    let body = r#"[{"draft":true,"tag_name":"v9.0.0"},{"prerelease":true,"tag_name":"v8.0.0-rc1"},{"tag_name":"v1.2.3"}]"#;
    assert_eq!(sniff_latest(source, body), Some("v1.2.3".to_owned()));
    assert_eq!(
        sniff_latest(
            "https://crates.io/api/v1/crates/example",
            r#"{"crate":{"max_version":"1.2.3"}}"#
        ),
        Some("1.2.3".to_owned())
    );
    assert_eq!(
        sniff_latest(source, r#"{"tag_name":"v2.3.4"}"#),
        Some("v2.3.4".to_owned())
    );
    assert_eq!(
        sniff_latest(
            "https://pypi.org/pypi/example/json",
            r#"{"info":{"version":"6.2.0"}}"#
        ),
        Some("6.2.0".to_owned())
    );
    assert_eq!(
        sniff_latest(
            "https://www.python.org/downloads/",
            "<a>Download Python 3.14.8</a>"
        ),
        Some("3.14.8".to_owned())
    );
    assert_eq!(
        sniff_latest(
            "https://static.rust-lang.org/dist/channel-rust-stable.toml",
            "[pkg.rust]\nversion = \"1.98.1 (abc 2026-10-01)\"\n"
        ),
        Some("1.98.1".to_owned())
    );
    assert_eq!(
        sniff_latest(source, r#"[{"draft":true,"tag_name":"v3.0.0"}]"#),
        None
    );
}

#[test]
fn corrupt_or_truncated_gzip_streams_fail_closed() {
    let mut corrupt = gzip(b"payload");
    let checksum_offset = corrupt.len().saturating_sub(8);
    if let Some(checksum) = corrupt.get_mut(checksum_offset) {
        *checksum ^= 1;
    }
    assert!(decode_body("gzip", &corrupt).is_err());
    let truncated = gzip(b"payload");
    assert!(
        decode_body(
            "gzip",
            truncated
                .get(..truncated.len().saturating_sub(1))
                .unwrap_or_default()
        )
        .is_err()
    );
}
