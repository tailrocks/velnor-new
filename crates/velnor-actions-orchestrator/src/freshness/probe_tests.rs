use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;

use flate2::Compression;
use flate2::write::GzEncoder;

use super::{FETCH_CAP, decode_body, fetch_text_with, sniff_latest, validate_probe_source};

#[test]
fn parses_stable_release_shapes_used_by_reviewed_sources() {
    assert_eq!(
        sniff_latest(
            "https://api.github.com/repos/acme/tool/releases/latest",
            r#"{"tag_name":"v2.3.4"}"#,
        ),
        Some("v2.3.4".to_owned())
    );
    assert_eq!(
        sniff_latest(
            "https://pypi.org/pypi/example/json",
            r#"{"info":{"version":"2.3.4"}}"#,
        ),
        Some("2.3.4".to_owned())
    );
    assert_eq!(
        sniff_latest(
            "https://crates.io/api/v1/crates/example",
            r#"{"crate":{"max_version":"0.12.23"}}"#,
        ),
        Some("0.12.23".to_owned())
    );
    assert_eq!(
        sniff_latest(
            "https://api.github.com/repos/acme/tool/releases",
            r#"[{"tag_name":"v9.0.0","draft":true},{"tag_name":"v8.0.0","prerelease":true},{"tag_name":"v2.3.4"}]"#,
        ),
        Some("v2.3.4".to_owned())
    );
    assert_eq!(
        sniff_latest(
            "https://www.python.org/downloads/",
            "<a>Download Python 2.3.4</a>",
        ),
        Some("2.3.4".to_owned())
    );
    assert_eq!(
        sniff_latest(
            "https://www.rust-lang.org/dist/channel-rust-stable.toml",
            "[pkg.rust]\nversion = \"1.98.1\"",
        ),
        Some("1.98.1".to_owned())
    );
}

#[test]
fn decodes_bounded_gzip_and_rejects_expansion_over_one_mib()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let headers = directory.path().join("headers");
    std::fs::write(
        &headers,
        "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n",
    )?;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(b"{\"tag_name\":\"v2.3.4\"}")?;
    let compressed = encoder.finish()?;
    assert_eq!(
        decode_body(&compressed, &headers)?,
        "{\"tag_name\":\"v2.3.4\"}"
    );

    let mut oversized = GzEncoder::new(Vec::new(), Compression::default());
    let expansion = vec![b'x'; FETCH_CAP + 1];
    oversized.write_all(&expansion)?;
    let compressed = oversized.finish()?;
    let error = decode_body(&compressed, &headers).expect_err("expanded body must remain capped");
    assert!(error.contains("decoded freshness response exceeds 1048576 bytes"));
    Ok(())
}

#[test]
fn rejects_unsupported_and_malformed_response_encodings() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let headers = directory.path().join("headers");
    std::fs::write(&headers, "HTTP/1.1 200 OK\r\nContent-Encoding: br\r\n\r\n")?;
    assert!(
        decode_body(b"body", &headers)
            .expect_err("unsupported response encoding must fail")
            .contains("unsupported Content-Encoding: br")
    );

    std::fs::write(
        &headers,
        "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n",
    )?;
    assert!(
        decode_body(b"not gzip", &headers)
            .expect_err("malformed gzip must fail")
            .contains("invalid gzip freshness response")
    );
    Ok(())
}

#[test]
fn rejects_truncated_and_corrupt_gzip_streams() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let headers = directory.path().join("headers");
    std::fs::write(
        &headers,
        "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n",
    )?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(b"payload")?;
    let valid = encoder.finish()?;
    let mut truncated_footer = valid.clone();
    truncated_footer.truncate(valid.len() - 4);
    let mut corrupt_crc = valid.clone();
    corrupt_crc[valid.len() - 8] ^= 1;
    for invalid in [
        Vec::new(),
        vec![0x1f, 0x8b, 0x08],
        truncated_footer,
        corrupt_crc,
    ] {
        assert!(
            decode_body(&invalid, &headers)
                .expect_err("invalid gzip stream must fail")
                .contains("invalid gzip freshness response")
        );
    }
    Ok(())
}

#[test]
fn decodes_gzip_html_before_parsing_python_release() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let headers = directory.path().join("headers");
    std::fs::write(
        &headers,
        "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n",
    )?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(b"<a>Download Python 3.13.7</a>")?;
    let body = decode_body(&encoder.finish()?, &headers)?;
    assert_eq!(
        sniff_latest("https://www.python.org/downloads/", &body),
        Some("3.13.7".to_owned())
    );
    Ok(())
}

#[test]
fn accepts_identity_and_gzip_bodies_at_the_one_mib_limit() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let headers = directory.path().join("headers");
    let body = vec![b'x'; FETCH_CAP];
    std::fs::write(&headers, "HTTP/1.1 200 OK\r\n\r\n")?;
    assert_eq!(decode_body(&body, &headers)?.len(), FETCH_CAP);
    let oversized = vec![b'x'; FETCH_CAP + 1];
    assert!(
        decode_body(&oversized, &headers)
            .expect_err("encoded identity body must remain capped")
            .contains("encoded freshness response exceeds 1048576 bytes")
    );

    std::fs::write(
        &headers,
        "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\n",
    )?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&body)?;
    let compressed = encoder.finish()?;
    assert_eq!(decode_body(&compressed, &headers)?.len(), FETCH_CAP);
    Ok(())
}

#[test]
fn fetches_file_fixture_and_rejects_invalid_sources() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let body = directory.path().join("latest.json");
    std::fs::write(&body, "{\"tag_name\":\"v2.3.4\"}")?;
    let url = format!("file://{}", body.display());
    assert_eq!(
        fetch_text_with(&url, Path::new("curl"), None).expect("file fixture fetch"),
        "{\"tag_name\":\"v2.3.4\"}"
    );
    let error = fetch_text_with(
        "--config=/tmp/untrusted-curlrc",
        Path::new("/unavailable/curl"),
        None,
    )
    .expect_err("option-looking value has no supported URL scheme");
    assert!(error.contains("unsupported or malformed source URL"));
    for source in [
        "ftp://example.invalid",
        "http://?missing-host",
        "https://#missing-host",
        "http://:8443/path",
        "http://[::1/path",
        "http://example.invalid/path\nheader: injected",
        "file://",
        "file://localhost",
        "file://remote.example/path",
    ] {
        assert!(
            validate_probe_source(source).is_err(),
            "accepted {source:?}"
        );
    }
    assert!(validate_probe_source("https://example.invalid/latest").is_ok());
    Ok(())
}

#[test]
fn rejects_non_success_http_status() -> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept local request");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).expect("read request") == 0 || line == "\r\n" {
                break;
            }
        }
        let body = "{\"tag_name\":\"v99.0.0\"}";
        write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("write response");
    });
    let url = format!("http://{address}/latest");
    let result = fetch_text_with(&url, Path::new("curl"), Some(home.path()));
    server.join().expect("server thread");
    assert!(result.is_err(), "HTTP error status must fail the probe");
    Ok(())
}

#[test]
fn ignores_hostile_curlrc_and_fetches_literal_local_http_url()
-> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    std::fs::write(home.path().join(".curlrc"), "header = \"X-Evil: yes\"\n")?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept local request");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut request = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).expect("read request") == 0 {
                break;
            }
            if line == "\r\n" {
                break;
            }
            request.push_str(&line);
        }
        let body = "{\"tag_name\":\"v2.3.4\"}";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("write response");
        request
    });
    let url = format!("http://{address}/latest/{{a,b}}/[1-2]");
    let body =
        fetch_text_with(&url, Path::new("curl"), Some(home.path())).expect("local HTTP fetch");
    let request = server.join().expect("server thread");
    assert_eq!(body, "{\"tag_name\":\"v2.3.4\"}");
    assert!(request.contains("GET /latest/{a,b}/[1-2] HTTP/1.1"));
    assert!(
        !request.to_ascii_lowercase().contains("x-evil:"),
        "ambient curl config changed the request: {request}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn passes_https_source_as_a_safe_curl_url() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir()?;
    let home = tempfile::tempdir()?;
    let curl = directory.path().join("curl");
    std::fs::write(
        &curl,
        "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$@\" > \"$HOME/args\"\n\
         headers= body=\n\
         while [ \"$#\" -gt 0 ]; do\n\
         case \"$1\" in\n\
         --dump-header) headers=$2; shift 2 ;;\n\
         --output) body=$2; shift 2 ;;\n\
         --url) shift 2 ;;\n\
         *) shift ;;\n\
         esac\n\
         done\n\
         printf 'HTTP/1.1 200 OK\\r\\n\\r\\n' > \"$headers\"\n\
         printf '{\"tag_name\":\"v2.3.4\"}' > \"$body\"\n",
    )?;
    let mut permissions = std::fs::metadata(&curl)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&curl, permissions)?;

    let url = "https://example.invalid/latest?next=--config=/tmp/untrusted-curlrc";
    assert_eq!(
        fetch_text_with(url, &curl, Some(home.path()))?,
        "{\"tag_name\":\"v2.3.4\"}"
    );
    let args = std::fs::read_to_string(home.path().join("args"))?;
    let args = args.lines().collect::<Vec<_>>();
    assert_eq!(args.first(), Some(&"--disable"));
    assert_eq!(args.get(1), Some(&"--globoff"));
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--proto" && pair[1] == "=http,https,file")
    );
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--proto-redir" && pair[1] == "=http,https")
    );
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--url" && pair[1] == url)
    );
    Ok(())
}
