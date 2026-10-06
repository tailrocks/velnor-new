//! Advisory-policy checks and bounded Cargo Deny subprocesses.

use std::fs;

use std::process::Command;

use std::time::Duration;

use toml::Value as TomlValue;

use crate::context::FreshnessContext;

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
        for advisory in ignored {
            ctx.fail_row(
                "advisories",
                &format!("{path}:{advisory}"),
                "ignored advisory must be a policy exception instead",
            );
        }
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
    use super::tail;

    #[test]
    fn diagnostic_tail_is_bounded_by_utf8_bytes() {
        assert_eq!(tail("0123456789", 4), "6789");
    }
}
