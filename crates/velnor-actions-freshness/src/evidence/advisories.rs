// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use std::process::Command;

use std::time::Duration;

use serde_json::Value;

use toml::Value as TomlValue;

use crate::context::{FreshnessContext, iso_date, parse_iso_date};

use crate::process::run_bounded;

const LIVE_SCAN_TIMEOUT: Duration = Duration::from_secs(180);

const PROCESS_OUTPUT_CAP: usize = 512 * 1024;

pub(crate) fn check_advisories(ctx: &mut FreshnessContext) {
    if ctx.workspace_roots.is_empty() {
        ctx.fail_row(
            "advisories",
            "(workspaces)",
            "no Cargo workspaces discovered",
        );
        return;
    }
    let workspaces = ctx.workspace_roots.clone();
    for (relative, _) in &workspaces {
        check_deny_policy(ctx, relative);
    }
    if ctx.with_advisories {
        for (relative, _) in &workspaces {
            check_live_advisories(ctx, relative);
        }
    } else {
        ctx.info_row(
            "advisories",
            "live scan",
            "runs as the CI Cargo Deny job; --with-advisories runs it here",
        );
    }
}

fn check_deny_policy(ctx: &mut FreshnessContext, relative: &str) {
    let path = if relative.is_empty() {
        "deny.toml".to_owned()
    } else {
        format!("{relative}/deny.toml")
    };
    let text = match fs::read_to_string(ctx.path(&path)) {
        Ok(text) => text,
        Err(error) => {
            ctx.fail_row("advisories", &path, &format!("unreadable ({error})"));
            return;
        }
    };
    let deny = match toml::from_str::<TomlValue>(&text) {
        Ok(deny) => deny,
        Err(error) => {
            ctx.fail_row("advisories", &path, &format!("unreadable ({error})"));
            return;
        }
    };
    let Some(advisories) = deny.get("advisories").and_then(TomlValue::as_table) else {
        ctx.fail_row("advisories", &path, "[advisories] table missing");
        return;
    };
    let Some(ignored) = advisories.get("ignore").and_then(TomlValue::as_array) else {
        ctx.fail_row(
            "advisories",
            &path,
            "[advisories].ignore must be an empty array",
        );
        return;
    };
    if ignored.is_empty() {
        ctx.pass_row("advisories", &format!("{path} ignore list"), "empty");
    } else {
        let today = ctx.now.div_euclid(86_400);
        for advisory in ignored {
            let subject = format!("{path}:{advisory}");
            let verdict = advisory_id(advisory).map_or_else(
                || Err("ignored advisory has no id".to_owned()),
                |id| hold_verdict(&ctx.holds, id, today, ctx.max_days),
            );
            match verdict {
                Ok(expires) => ctx.pass_row(
                    "advisories",
                    &subject,
                    &format!("ignored under a dated hold expiring {expires}"),
                ),
                Err(reason) => ctx.fail_row("advisories", &subject, &reason),
            }
        }
    }
}

pub(crate) fn advisory_id(entry: &TomlValue) -> Option<&str> {
    entry
        .as_str()
        .or_else(|| entry.get("id").and_then(TomlValue::as_str))
}

pub(crate) fn ignored_advisory_ids(ctx: &FreshnessContext) -> Vec<String> {
    let mut ids = Vec::new();
    for (relative, _) in &ctx.workspace_roots {
        let path = if relative.is_empty() {
            "deny.toml".to_owned()
        } else {
            format!("{relative}/deny.toml")
        };
        let Some(deny) = fs::read_to_string(ctx.path(&path))
            .ok()
            .and_then(|text| toml::from_str::<TomlValue>(&text).ok())
        else {
            continue;
        };
        ids.extend(
            deny.get("advisories")
                .and_then(|advisories| advisories.get("ignore"))
                .and_then(TomlValue::as_array)
                .into_iter()
                .flatten()
                .filter_map(advisory_id)
                .map(str::to_owned),
        );
    }
    ids
}

fn hold_verdict(holds: &[Value], id: &str, today: i64, max_days: i64) -> Result<String, String> {
    let missing =
        || format!("ignored advisory {id} needs a dated temporary_holds exception (key {id})");
    let hold = holds
        .iter()
        .find(|hold| hold.get("key").and_then(Value::as_str) == Some(id))
        .ok_or_else(missing)?;
    let date = |field: &str| {
        hold.get(field)
            .and_then(Value::as_str)
            .and_then(parse_iso_date)
    };
    let (Some(granted), Some(expires)) = (date("granted"), date("expires")) else {
        return Err(format!("hold for {id} lacks YYYY-MM-DD granted/expires"));
    };
    if expires - granted > max_days {
        Err(format!("hold for {id} spans more than {max_days}d"))
    } else if expires < today {
        Err(format!("hold for {id} expired {}", iso_date(expires)))
    } else {
        Ok(iso_date(expires))
    }
}

fn check_live_advisories(ctx: &mut FreshnessContext, relative: &str) {
    let manifest = if relative.is_empty() {
        ctx.path("Cargo.toml")
    } else {
        ctx.path(relative).join("Cargo.toml")
    };
    let config = if relative.is_empty() {
        ctx.path("deny.toml")
    } else {
        ctx.path(relative).join("deny.toml")
    };
    let subject = manifest.strip_prefix(&ctx.root).map_or_else(
        |_| manifest.display().to_string(),
        |path| path.display().to_string(),
    );
    match run_cargo_deny(&ctx.root, &manifest, &config) {
        Ok((true, _)) => ctx.pass_row("advisories", &subject, "cargo deny check advisories: no findings"),
        Ok((false, tail)) => ctx.fail_row("advisories", &subject, &format!("cargo deny reported findings; run `cargo deny check advisories` for evidence: {tail:?}")),
        Err(error) => ctx.fail_row("advisories", &subject, &format!("live scan failed ({error})")),
    }
}

fn run_cargo_deny(
    root: &std::path::Path,
    manifest: &std::path::Path,
    config: &std::path::Path,
) -> Result<(bool, String), String> {
    let output = run_bounded(
        Command::new("cargo-deny")
            .args(["--locked", "--config"])
            .arg(config)
            .arg("--manifest-path")
            .arg(manifest)
            .args(["check", "advisories"])
            .current_dir(root),
        PROCESS_OUTPUT_CAP,
        LIVE_SCAN_TIMEOUT,
    )
    .map_err(|error| format!("cargo-deny process failed ({error})"))?;
    let captured = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok((output.status.success(), tail(&captured, 500)))
}

fn tail(value: &str, cap: usize) -> String {
    let bytes = value.as_bytes();
    let start = bytes.len().saturating_sub(cap);
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::hold_verdict;

    fn holds() -> Vec<serde_json::Value> {
        vec![json!({"key": "RUSTSEC-2026-0543", "granted": "2026-11-19", "expires": "2026-12-03"})]
    }

    const NOV_20: i64 = 20_777;

    #[test]
    fn held_ignore_passes() {
        let today = crate::context::parse_iso_date("2026-11-20").unwrap_or(NOV_20);
        assert!(hold_verdict(&holds(), "RUSTSEC-2026-0543", today, 14).is_ok());
    }

    #[test]
    fn unheld_ignore_fails() {
        let today = crate::context::parse_iso_date("2026-11-20").unwrap_or(NOV_20);
        assert!(hold_verdict(&holds(), "RUSTSEC-2026-9999", today, 14).is_err());
    }

    #[test]
    fn expired_hold_fails() {
        let today = crate::context::parse_iso_date("2026-12-04").unwrap_or(NOV_20);
        assert!(hold_verdict(&holds(), "RUSTSEC-2026-0543", today, 14).is_err());
    }
}
