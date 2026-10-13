//! Upstream release selection for the optional scheduled freshness probe.

use serde_json::Value;

use crate::context::{FreshnessContext, iso_timestamp, norm_version};
use transport::{fetch_prefix_text, fetch_text};

const RUST_STABLE_MANIFEST_SOURCE: &str =
    "https://static.rust-lang.org/dist/channel-rust-stable.toml";
const RUST_STABLE_MANIFEST_PREFIX_BYTES: usize = 128 * 1024;

mod transport;

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
            .pointer("/crate/max_version")?
            .as_str()
            .map(str::to_owned);
    }
    if let Some(version) = payload.pointer("/info/version").and_then(Value::as_str)
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
        .pointer("/crate/max_version")?
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
    let mut rust_table = None::<String>;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if rust_table.is_some() {
                break;
            }
            if trimmed == "[pkg.rust]" {
                rust_table = Some("[pkg.rust]\n".to_owned());
            }
        } else if let Some(table) = &mut rust_table {
            table.push_str(line);
            table.push('\n');
        }
    }
    let document = rust_table?.parse::<toml::Table>().ok()?;
    let version = document
        .get("pkg")?
        .get("rust")?
        .get("version")?
        .as_str()?
        .split_whitespace()
        .next()?;
    semantic_version(version).then(|| version.to_owned())
}

fn semantic_version(text: &str) -> bool {
    let mut parts = text.split('.');
    [parts.next(), parts.next(), parts.next()]
        .into_iter()
        .all(|part| {
            part.is_some_and(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
        })
        && parts.next().is_none()
}

/// Run one optional upstream lookup for every inventoried tool and action.
pub(crate) fn check_upstream_probe(ctx: &mut FreshnessContext) {
    let stamp = iso_timestamp(ctx.now);
    for tool in ctx.tools.clone() {
        probe_pin(
            ctx,
            &value_text(&tool, "name", "<missing>"),
            &value_text(&tool, "source", ""),
            &value_text(&tool, "pinned", ""),
            &stamp,
        );
    }
    let actions = ctx.action_pinned.clone();
    for (subject, action) in actions {
        probe_pin(
            ctx,
            &subject,
            &value_text(&action, "source", ""),
            &value_text(&action, "pinned_version", ""),
            &stamp,
        );
    }
    ctx.info_row(
        "upstream-probe",
        "runner",
        "latest image family is platform-qualification evidence, not an API probe",
    );
}

fn value_text(value: &Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_owned()
}

fn probe_pin(ctx: &mut FreshnessContext, subject: &str, source: &str, pinned: &str, stamp: &str) {
    let response = if source == RUST_STABLE_MANIFEST_SOURCE {
        fetch_prefix_text(source, RUST_STABLE_MANIFEST_PREFIX_BYTES)
    } else {
        fetch_text(source)
    };
    let result = response.and_then(|body| {
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
