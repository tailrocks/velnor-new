//! Bounded upstream release lookups for the optional scheduled probe.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use flate2::read::MultiGzDecoder;
use serde_json::Value;

use crate::context::{FETCH_CAP, FreshnessContext, iso_timestamp, norm_version};

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
static UPSTREAM_AGENT: OnceLock<Result<ureq::Agent, String>> = OnceLock::new();

/// Fetch text while bounding both wire and decompressed response bodies.
pub(crate) fn fetch_text(url: &str) -> Result<String, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return fetch_file(path);
    }
    let agent = upstream_agent()?;
    let mut response = agent
        .get(url)
        .header("User-Agent", "velnor-freshness-probe")
        .header("Accept", "application/json")
        .header("Accept-Encoding", "gzip")
        .call()
        .map_err(|error| error.to_string())?;
    let encoding = response
        .headers()
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("identity")
        .trim()
        .to_ascii_lowercase();
    let mut encoded = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(
            u64::try_from(FETCH_CAP)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut encoded)
        .map_err(|error| format!("response read failed ({error})"))?;
    if encoded.len() > FETCH_CAP {
        return Err(format!(
            "encoded freshness response exceeds {FETCH_CAP} bytes"
        ));
    }
    let decoded = decode_body(&encoding, &encoded)?;
    if decoded.len() > FETCH_CAP {
        return Err(format!(
            "decoded freshness response exceeds {FETCH_CAP} bytes"
        ));
    }
    Ok(String::from_utf8_lossy(&decoded).into_owned())
}

fn upstream_agent() -> Result<&'static ureq::Agent, String> {
    UPSTREAM_AGENT
        .get_or_init(build_upstream_agent)
        .as_ref()
        .map_err(Clone::clone)
}

fn build_upstream_agent() -> Result<ureq::Agent, String> {
    let trust = rustls_native_certs::load_native_certs();
    if !trust.errors.is_empty() {
        return Err("platform trust certificates could not all be loaded".to_owned());
    }
    if trust.certs.is_empty() {
        return Err("no platform trust certificates were loaded".to_owned());
    }
    let roots = trust
        .certs
        .iter()
        .map(|certificate| ureq::tls::Certificate::from_der(certificate.as_ref()).to_owned())
        .collect::<Vec<_>>();
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(roots.into())
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let config = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(FETCH_TIMEOUT))
        .max_redirects(5)
        .build();
    Ok(ureq::Agent::new_with_config(config))
}

fn fetch_file(path: &str) -> Result<String, String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("file source path must be absolute".to_owned());
    }
    let file = File::open(path).map_err(|error| format!("file source could not open ({error})"))?;
    let mut bytes = Vec::new();
    file.take(
        u64::try_from(FETCH_CAP)
            .unwrap_or(u64::MAX)
            .saturating_add(1),
    )
    .read_to_end(&mut bytes)
    .map_err(|error| format!("file source read failed ({error})"))?;
    if bytes.len() > FETCH_CAP {
        return Err(format!("file freshness source exceeds {FETCH_CAP} bytes"));
    }
    String::from_utf8(bytes)
        .map_err(|error| format!("file freshness source is not UTF-8 ({error})"))
}

fn decode_body(encoding: &str, encoded: &[u8]) -> Result<Vec<u8>, String> {
    match encoding {
        "" | "identity" => Ok(encoded.to_vec()),
        "gzip" => {
            let gzip_reader = MultiGzDecoder::new(encoded);
            let mut decoded_body = Vec::new();
            gzip_reader
                .take(
                    u64::try_from(FETCH_CAP)
                        .unwrap_or(u64::MAX)
                        .saturating_add(1),
                )
                .read_to_end(&mut decoded_body)
                .map_err(|error| format!("invalid gzip freshness response ({error})"))?;
            Ok(decoded_body)
        }
        _ => Err(format!("unsupported Content-Encoding: {encoding}")),
    }
}

/// Select the latest stable release value from the known upstream formats.
pub(crate) fn sniff_latest(source: &str, body: &str) -> Option<String> {
    if let Ok(payload) = serde_json::from_str::<Value>(body)
        && let Some(release) = json_release(source, &payload)
    {
        return Some(release);
    }
    html_release(body)
}

fn json_release(source: &str, payload: &Value) -> Option<String> {
    if source.contains("crates.io/api/v1/crates/") {
        return payload
            .get("crate")?
            .get("max_version")?
            .as_str()
            .map(str::to_owned);
    }
    if let Some(version) = payload
        .get("info")
        .and_then(|info| info.get("version"))
        .and_then(Value::as_str)
        && semantic_version(version)
    {
        return Some(version.to_owned());
    }
    if let Some(tag) = payload.get("tag_name").and_then(Value::as_str) {
        return Some(tag.to_owned());
    }
    if let Some(releases) = payload.as_array() {
        for release in releases {
            if release.get("draft").and_then(Value::as_bool) == Some(true)
                || release.get("prerelease").and_then(Value::as_bool) == Some(true)
            {
                continue;
            }
            if let Some(tag) = release
                .get("tag_name")
                .or_else(|| release.get("name"))
                .and_then(Value::as_str)
            {
                return Some(tag.to_owned());
            }
        }
    }
    payload
        .get("crate")?
        .get("max_version")?
        .as_str()
        .map(str::to_owned)
}

fn html_release(body: &str) -> Option<String> {
    if let Some((_, tail)) = body.split_once(">Download Python ")
        && let Some((version, _)) = tail.split_once('<')
        && semantic_version(version)
    {
        return Some(version.to_owned());
    }
    if let Some((_, tail)) = body.split_once("[pkg.rust]")
        && let Some(version) = version_assignment(tail)
    {
        return Some(version);
    }
    version_assignment(body)
}

fn version_assignment(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        if key.trim() != "version" {
            continue;
        }
        let value = value
            .trim()
            .strip_prefix('"')?
            .split('"')
            .next()?
            .split_whitespace()
            .next()?;
        if semantic_version(value) {
            return Some(value.to_owned());
        }
    }
    None
}

fn semantic_version(text: &str) -> bool {
    let mut parts = text.split('.');
    let major = parts.next();
    let minor = parts.next();
    let patch = parts.next();
    [major, minor, patch].into_iter().all(|part| {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none()
}

/// Run one optional upstream lookup for every inventoried tool and action.
pub(crate) fn check_upstream_probe(ctx: &mut FreshnessContext) {
    let stamp = iso_timestamp(ctx.now);
    for tool in ctx.tools.clone() {
        let subject = tool
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<missing>")
            .to_owned();
        let source = tool
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let pinned = tool
            .get("pinned")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        probe_pin(ctx, &subject, &source, &pinned, &stamp);
    }
    let actions = ctx
        .action_pinned
        .iter()
        .map(|(key, action)| (key.clone(), action.clone()))
        .collect::<Vec<_>>();
    for (subject, action) in actions {
        let source = action
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let pinned = action
            .get("pinned_version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        probe_pin(ctx, &subject, &source, &pinned, &stamp);
    }
    ctx.info_row(
        "upstream-probe",
        "runner",
        "latest image family is platform-qualification evidence, not an API probe",
    );
}

fn probe_pin(ctx: &mut FreshnessContext, subject: &str, source: &str, pinned: &str, stamp: &str) {
    let result = fetch_text(source).and_then(|body| {
        sniff_latest(source, &body).ok_or_else(|| "no stable release parsed".to_owned())
    });
    match result {
        Err(error) => ctx.fail_row(
            "upstream-probe",
            subject,
            &format!("lookup_failed ({error}); source {source}, checked {stamp}"),
        ),
        Ok(latest) if norm_version(&latest) != norm_version(pinned) => ctx.fail_row(
            "upstream-probe",
            subject,
            &format!(
                "stale pin: pinned={pinned:?} latest={latest:?}; source {source}, checked {stamp}"
            ),
        ),
        Ok(latest) => ctx.pass_row(
            "upstream-probe",
            subject,
            &format!("pinned==latest {latest}; source {source}, checked {stamp}"),
        ),
    }
}

#[cfg(test)]
mod tests;
