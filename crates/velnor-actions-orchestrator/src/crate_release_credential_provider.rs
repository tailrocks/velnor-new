// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(unix)]
use std::io::BufReader;
use std::io::{BufRead, Read, Write};

pub fn run() -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{{\"v\":[1]}}")?;
    stdout.flush()?;
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    loop {
        let mut line = String::new();
        if input.by_ref().take(8193).read_line(&mut line)? == 0 {
            break;
        }
        if line.len() > 8192 || !line.ends_with('\n') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "credential request invalid",
            ));
        }
        let request: serde_json::Value =
            serde_json::from_str(&line).map_err(std::io::Error::other)?;
        let response = respond(&request);
        serde_json::to_writer(&mut stdout, &response).map_err(std::io::Error::other)?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
    Ok(())
}

fn respond(request: &serde_json::Value) -> serde_json::Value {
    let invalid = || serde_json::json!({"Err":{"kind":"not-found"}});
    if request["v"] != 1 || request["kind"] != "get" {
        return invalid();
    }
    let registry = request["registry"]["index-url"]
        .as_str()
        .unwrap_or_default();
    if !matches!(
        registry,
        "sparse+https://index.crates.io/" | "https://github.com/rust-lang/crates.io-index"
    ) {
        return invalid();
    }
    let token = match request["operation"].as_str() {
        Some("read") => "velnor-nonsecret-read-placeholder".to_owned(),
        Some("publish") if authorize(request) => match std::env::var("CARGO_REGISTRY_TOKEN") {
            Ok(token) if !token.is_empty() => token,
            _ => return invalid(),
        },
        _ => return invalid(),
    };
    serde_json::json!({"Ok":{"kind":"get","token":token,"cache":"never","operation_independent":false}})
}

#[cfg(unix)]
fn authorize(request: &serde_json::Value) -> bool {
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    let Ok(path) = std::env::var("VELNOR_CARGO_AUTH_SOCKET") else {
        return false;
    };
    let Ok(mut stream) = UnixStream::connect(path) else {
        return false;
    };
    if stream
        .set_read_timeout(Some(Duration::from_secs(600)))
        .is_err()
        || stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .is_err()
    {
        return false;
    }
    if serde_json::to_writer(&mut stream, request).is_err() || stream.write_all(b"\n").is_err() {
        return false;
    }
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).is_ok() && response == "allow\n"
}

#[cfg(not(unix))]
fn authorize(_: &serde_json::Value) -> bool {
    false
}
