//! Bounded upstream lookup for reviewed tool and action pins.

use std::io::Read;
use std::path::Path;
use std::process::Command;

use flate2::read::MultiGzDecoder;
use serde_json::Value;

use super::context::{FreshnessContext, format_utc, norm_version, py_repr};

const FETCH_TIMEOUT_SECS: &str = "10";
const FETCH_CAP: usize = 1024 * 1024;

pub(super) fn check_upstream_probe(ctx: &mut FreshnessContext) {
    let stamp = format_utc(ctx.now_micros);
    for tool in ctx.tools.clone() {
        probe_pin(
            ctx,
            tool.get("name"),
            tool.get("source").and_then(Value::as_str).unwrap_or(""),
            tool.get("pinned"),
            &stamp,
        );
    }
    let actions: Vec<(String, Value)> = ctx
        .action_pinned
        .iter()
        .map(|(key, action)| (key.clone(), action.clone()))
        .collect();
    for (key, action) in actions {
        probe_pin(
            ctx,
            Some(&Value::String(key)),
            action.get("source").and_then(Value::as_str).unwrap_or(""),
            action.get("pinned_version"),
            &stamp,
        );
    }
    ctx.info_row(
        "upstream-probe",
        "runner",
        "latest image family is platform-qualification evidence, not an API probe",
    );
}

fn probe_pin(
    ctx: &mut FreshnessContext,
    subject: Option<&Value>,
    source: &str,
    pinned: Option<&Value>,
    stamp: &str,
) {
    let subject = subject
        .and_then(Value::as_str)
        .map_or_else(|| py_repr(subject), str::to_owned);
    let fetched = fetch_text(source);
    let latest = match fetched {
        Ok(body) => sniff_latest(source, &body),
        Err(error) => {
            ctx.fail_row(
                "upstream-probe",
                &subject,
                &format!("lookup_failed ({error}); source {source}, checked {stamp}"),
            );
            return;
        }
    };
    let Some(latest) = latest else {
        ctx.fail_row(
            "upstream-probe",
            &subject,
            &format!("lookup_failed: no stable release parsed; source {source}, checked {stamp}"),
        );
        return;
    };
    let pinned_version = norm_version(pinned);
    if norm_version(Some(&Value::String(latest.clone()))) == pinned_version {
        ctx.pass_row(
            "upstream-probe",
            &subject,
            &format!("pinned==latest {latest}; source {source}, checked {stamp}"),
        );
    } else {
        ctx.fail_row(
            "upstream-probe",
            &subject,
            &format!(
                "stale pin: pinned={} latest={}; source {source}, checked {stamp}",
                py_repr(pinned),
                py_repr(Some(&Value::String(latest)))
            ),
        );
    }
}

fn fetch_text(url: &str) -> Result<String, String> {
    fetch_text_with(url, Path::new("curl"), None)
}

fn fetch_text_with(url: &str, curl_program: &Path, home: Option<&Path>) -> Result<String, String> {
    validate_probe_source(url)?;
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let headers_path = directory.path().join("headers");
    let body_path = directory.path().join("body");
    let mut command = Command::new(curl_program);
    if let Some(home) = home {
        command.env("HOME", home).env_remove("CURL_HOME");
        for key in [
            "ALL_PROXY",
            "all_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
        ] {
            command.env_remove(key);
        }
    }
    let output = command
        .args([
            "--disable",
            "--globoff",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--max-redirs",
            "10",
            "--max-time",
            FETCH_TIMEOUT_SECS,
            "--max-filesize",
            "1048577",
            "--proto",
            "=http,https,file",
            "--proto-redir",
            "=http,https",
            "--header",
            "User-Agent: velnor-freshness-probe",
            "--header",
            "Accept: application/json",
            "--header",
            "Accept-Encoding: gzip",
            "--dump-header",
        ])
        .arg(&headers_path)
        .arg("--output")
        .arg(&body_path)
        .arg("--url")
        .arg(url)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if detail.is_empty() {
            format!("curl exit code {}", output.status.code().unwrap_or(-1))
        } else {
            detail
        });
    }
    let encoded = std::fs::read(&body_path).map_err(|error| error.to_string())?;
    decode_body(&encoded, &headers_path)
}

fn validate_probe_source(url: &str) -> Result<(), String> {
    if url
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("source URL contains whitespace or control characters".to_owned());
    }
    let Some((scheme, remainder)) = url.split_once("://") else {
        return Err("unsupported or malformed source URL".to_owned());
    };
    let is_http = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    let is_file = scheme.eq_ignore_ascii_case("file");
    if !is_http && !is_file {
        return Err(format!("unsupported source scheme: {scheme}"));
    }
    if is_http {
        let authority = remainder.split(['/', '?', '#']).next().unwrap_or("");
        if http_host(authority).is_none() {
            return Err("HTTP source URL has no host".to_owned());
        }
    }
    if is_file && !has_local_file_path(remainder) {
        return Err("file source URL has no local path".to_owned());
    }
    Ok(())
}

fn http_host(authority: &str) -> Option<&str> {
    let host_port = authority.rsplit('@').next()?;
    if let Some(bracketed) = host_port.strip_prefix('[') {
        let (host, suffix) = bracketed.split_once(']')?;
        return (!host.is_empty() && (suffix.is_empty() || suffix.starts_with(':')))
            .then_some(host);
    }
    if host_port.contains('[') || host_port.contains(']') {
        return None;
    }
    let host = host_port.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

fn has_local_file_path(remainder: &str) -> bool {
    remainder.starts_with('/')
        || remainder
            .split_once('/')
            .is_some_and(|(host, _)| host.eq_ignore_ascii_case("localhost"))
}

fn decode_body(encoded: &[u8], headers_path: &std::path::Path) -> Result<String, String> {
    if encoded.len() > FETCH_CAP {
        return Err(format!(
            "encoded freshness response exceeds {FETCH_CAP} bytes"
        ));
    }
    let headers = std::fs::read_to_string(headers_path).map_err(|error| error.to_string())?;
    let encoding = final_content_encoding(&headers);
    let decoded = match encoding.as_str() {
        "" | "identity" => encoded.to_vec(),
        "gzip" => decompress(encoded)?,
        _ => return Err(format!("unsupported Content-Encoding: {encoding}")),
    };
    if decoded.len() > FETCH_CAP {
        return Err(format!(
            "decoded freshness response exceeds {FETCH_CAP} bytes"
        ));
    }
    Ok(String::from_utf8_lossy(&decoded).into_owned())
}

fn final_content_encoding(headers: &str) -> String {
    let mut encoding = String::new();
    for line in headers.lines() {
        if line.starts_with("HTTP/") {
            encoding.clear();
        } else if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case("content-encoding")
        {
            encoding = value.trim().to_ascii_lowercase();
        }
    }
    encoding
}

fn decompress(encoded: &[u8]) -> Result<Vec<u8>, String> {
    let gzip_stream = MultiGzDecoder::new(encoded);
    let mut decoded = Vec::new();
    gzip_stream
        .take((FETCH_CAP + 1) as u64)
        .read_to_end(&mut decoded)
        .map_err(|error| format!("invalid gzip freshness response ({error})"))?;
    Ok(decoded)
}

fn sniff_latest(source: &str, body: &str) -> Option<String> {
    if let Ok(payload) = serde_json::from_str::<Value>(body)
        && let Some(version) = json_release(source, &payload)
    {
        return Some(version);
    }
    text_release(body)
}

fn json_release(source: &str, payload: &Value) -> Option<String> {
    if source.contains("crates.io/api/v1/crates/")
        && let Some(version) = payload
            .get("crate")
            .and_then(|crate_info| crate_info.get("max_version"))
            .and_then(Value::as_str)
    {
        return Some(version.to_owned());
    }
    if let Some(version) = payload
        .get("info")
        .and_then(|info| info.get("version"))
        .and_then(Value::as_str)
        .filter(|version| exact_semver(version))
    {
        return Some(version.to_owned());
    }
    if let Some(version) = github_tag(payload) {
        return Some(version);
    }
    payload
        .get("crate")
        .and_then(|crate_info| crate_info.get("max_version"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn github_tag(payload: &Value) -> Option<String> {
    if let Some(tag) = payload
        .get("tag_name")
        .and_then(Value::as_str)
        .filter(|tag| !tag.is_empty())
    {
        return Some(tag.to_owned());
    }
    payload.as_array()?.iter().find_map(|release| {
        if truthy(release.get("draft")) || truthy(release.get("prerelease")) {
            return None;
        }
        release
            .get("tag_name")
            .or_else(|| release.get("name"))
            .and_then(Value::as_str)
            .filter(|tag| !tag.is_empty())
            .map(str::to_owned)
    })
}

fn text_release(body: &str) -> Option<String> {
    rust_channel_release(body).or_else(|| find_assignment_semver(body))
}

fn rust_channel_release(body: &str) -> Option<String> {
    let section = body.find("[pkg.rust]")? + "[pkg.rust]".len();
    let remaining = &body[section..];
    let newline = remaining.find('\n')?;
    if !remaining[..newline].trim().is_empty() {
        return None;
    }
    find_assignment_semver(&remaining[newline + 1..])
}

fn find_assignment_semver(body: &str) -> Option<String> {
    for line in body.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "version" {
            continue;
        }
        let Some(value) = value.trim().strip_prefix('"') else {
            continue;
        };
        let candidate: String = value
            .chars()
            .take_while(|character| character.is_ascii_digit() || *character == '.')
            .collect();
        if exact_semver(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn exact_semver(value: &str) -> bool {
    let mut parts = value.split('.');
    (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none()
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
