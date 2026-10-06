use super::*;

#[test]
fn bounded_fetch_accepts_identity_gzip_and_chunked() {
    assert_body(mock_response(200, b"payload".to_vec()), "payload");
    assert_body(
        MockResponse {
            encoding: "gzip",
            body: gzip(b"payload"),
            ..mock_response(200, Vec::new())
        },
        "payload",
    );
}

#[test]
fn encoded_and_decoded_body_caps_are_enforced() {
    assert!(fetch_once(mock_response(200, vec![b'x'; FETCH_CAP + 1])).is_err());
    assert!(
        fetch_once(MockResponse {
            encoding: "gzip",
            body: gzip(&vec![b'x'; FETCH_CAP + 1]),
            ..mock_response(200, Vec::new())
        })
        .is_err()
    );
}

#[test]
fn intermediate_identity_gzip_and_chunked_bodies_obey_caps() {
    redirect_cap_case(b'i', "identity", false);
    redirect_cap_case(b'g', "gzip", false);
    redirect_cap_case(b'c', "identity", true);
}

#[test]
fn redirects_are_bounded_and_relative_locations_resolve() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        resolve_redirect("https://example.test/a/b?old=1", "../next?q=2"),
        Ok("https://example.test/next?q=2".to_owned())
    );
    assert_eq!(
        resolve_redirect("https://example.test/a/b", "?next=1"),
        Ok("https://example.test/a/b?next=1".to_owned())
    );
    for (base, location, expected) in [
        ("https://example.test/a/b", ".", "https://example.test/a/"),
        (
            "https://example.test/a/b/c",
            "..",
            "https://example.test/a/",
        ),
        (
            "https://example.test/a/b",
            "/releases//latest",
            "https://example.test/releases//latest",
        ),
        (
            "https://example.test/a/b",
            "/releases/latest/.",
            "https://example.test/releases/latest/",
        ),
        (
            "https://example.test/a/b",
            "/releases/latest/..",
            "https://example.test/releases/",
        ),
    ] {
        assert_eq!(resolve_redirect(base, location), Ok(expected.to_owned()));
    }
    assert!(resolve_redirect("https://example.test/a", "file:///etc/passwd").is_err());
    assert!(resolve_redirect("https://example.test/a", "mailto:ops@example.test").is_err());
    let responses = (0..MAX_REDIRECTS)
        .map(|_| redirect_response(Vec::new(), "identity", false))
        .chain(std::iter::once(mock_response(200, b"final".to_vec())))
        .collect();
    let server = start_server(responses)?;
    assert_eq!(
        fetch_http_with_agent(&agent(), &server.url, Duration::from_secs(2)),
        Ok("final".to_owned())
    );
    assert_eq!(server.finish(), MAX_REDIRECTS + 1);
    let responses = (0..=MAX_REDIRECTS)
        .map(|_| redirect_response(Vec::new(), "identity", false))
        .collect();
    let server = start_server(responses)?;
    assert!(fetch_http_with_agent(&agent(), &server.url, Duration::from_secs(2)).is_err());
    assert_eq!(server.finish(), MAX_REDIRECTS + 1);
    Ok(())
}

#[test]
fn redirect_sequence_uses_one_overall_deadline() -> Result<(), Box<dyn Error>> {
    let mut first = redirect_response(Vec::new(), "identity", false);
    first.delay = Duration::from_millis(90);
    let mut second = mock_response(200, b"late".to_vec());
    second.delay = Duration::from_millis(90);
    let server = start_server(vec![first, second])?;
    assert!(fetch_http_with_agent(&agent(), &server.url, Duration::from_millis(150)).is_err());
    assert_eq!(server.finish(), 2);
    Ok(())
}

#[test]
fn unsupported_and_broken_content_encodings_fail_closed() -> Result<(), Box<dyn Error>> {
    assert!(decode_body("br", b"body").is_err());
    assert!(decode_body("gzip", b"\x1f\x8b\x08").is_err());
    let server = start_server(vec![MockResponse {
        encoding: "br",
        body: vec![1, 2, 3],
        ..mock_response(200, Vec::new())
    }])?;
    assert!(fetch_text(&server.url).is_err());
    assert_eq!(server.finish(), 1);
    let mut corrupt = gzip(b"payload");
    let checksum_offset = corrupt.len().saturating_sub(8);
    if let Some(checksum) = corrupt.get_mut(checksum_offset) {
        *checksum ^= 1;
    }
    assert!(decode_body("gzip", &corrupt).is_err());
    let truncated = gzip(b"payload");
    assert!(decode_body("gzip", &truncated[..truncated.len().saturating_sub(1)]).is_err());
    Ok(())
}

#[test]
fn file_sources_require_absolute_regular_utf8_files() -> Result<(), Box<dyn Error>> {
    let directory = temp_dir()?;
    let regular = directory.0.join("source.toml");
    fs::write(&regular, vec![b'a'; FETCH_CAP])?;
    assert!(fetch_text(&file_url(&regular)).is_ok_and(|body| body.len() == FETCH_CAP));
    fs::write(&regular, vec![b'a'; FETCH_CAP + 1])?;
    assert!(fetch_text(&file_url(&regular)).is_err());
    fs::write(&regular, [0xff, 0xfe])?;
    assert!(fetch_text(&file_url(&regular)).is_err());
    assert!(fetch_text("file://relative/source.toml").is_err());
    assert!(file_fetch_watchdog(file_url(&directory.0)).is_ok_and(|result| result.is_err()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        use std::process::Command;

        let link = directory.0.join("link");
        symlink(&regular, &link)?;
        assert!(file_fetch_watchdog(file_url(&link)).is_ok_and(|result| result.is_err()));
        let fifo = directory.0.join("fifo");
        assert!(Command::new("mkfifo").arg(&fifo).status()?.success());
        assert!(file_fetch_watchdog(file_url(&fifo)).is_ok_and(|result| result.is_err()));
        let device = PathBuf::from("/dev/null");
        assert!(file_fetch_watchdog(file_url(&device)).is_ok_and(|result| result.is_err()));
    }
    directory.cleanup()?;
    Ok(())
}

#[test]
fn release_formats_parse_json_python_and_structured_rust_toml() {
    let source = "https://api.github.com/repos/example/tool/releases/latest";
    let cases = [
        (
            source,
            r#"[{"draft":true,"tag_name":"v9.0.0"},{"prerelease":true,"tag_name":"v8.0.0-rc1"},{"tag_name":"v1.2.3"}]"#,
            Some("v1.2.3"),
        ),
        (
            "https://crates.io/api/v1/crates/example",
            r#"{"crate":{"max_version":"1.2.3"}}"#,
            Some("1.2.3"),
        ),
        (source, r#"{"tag_name":"v2.3.4"}"#, Some("v2.3.4")),
        (
            "https://pypi.org/pypi/example/json",
            r#"{"info":{"version":"6.2.0"}}"#,
            Some("6.2.0"),
        ),
        (
            "https://www.python.org/downloads/",
            "<a>Download Python 3.14.8</a>",
            Some("3.14.8"),
        ),
        (
            "https://static.rust-lang.org/dist/channel-rust-stable.toml",
            "[pkg.rust]\nversion = \"1.98.1 (abc 2026-10-01)\"\n",
            Some("1.98.1"),
        ),
        (source, r#"[{"draft":true,"tag_name":"v3.0.0"}]"#, None),
    ];
    for (source, body, expected) in cases {
        assert_eq!(sniff_latest(source, body), expected.map(str::to_owned));
    }
    let decoys = [
        "version = \"1.98.1\"",
        "[pkg]\nversion = \"1.98.1\"",
        "[pkg.rust]\nname = \"rust\"\n[other]\nversion = \"1.98.1\"",
        "# [pkg.rust]\n# version = \"1.98.1\"\n[other]\nname = \"x\"",
        "decoy = \"[pkg.rust] version = 1.98.1\"",
        "[pkg.rust]\nversion = 1.98.1",
        "[pkg.rust]\nversion = \"1.98.1",
    ];
    assert!(
        decoys
            .iter()
            .all(|body| sniff_latest(source, body).is_none())
    );
}
