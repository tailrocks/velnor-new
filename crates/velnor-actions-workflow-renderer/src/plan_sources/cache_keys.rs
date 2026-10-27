// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    CACHE_KEYS_STEP_NAME, CARGO_HOME_EXPR, CacheFamily, RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL,
    SOURCES_KEY_OUTPUT, SOURCES_RESTORE_KEYS_OUTPUT, SOURCES_SAVE_OUTPUT, SOURCES_SNAPSHOT_OUTPUT,
    TRUST_OUTPUT, tofu_providers_key_output,
};
use velnor_actions_contract::{Step, StepId, StepRole, validate_fetch_root};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use super::PlanSourcesError;

pub const DEFAULT_REF_ENV: &str = "VELNOR_DEFAULT_REF";
const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";
const OUTPUT_DELIMITER: &str = "VELNOR_CACHE_KEYS_EOF";
const MAX_KEY: usize = velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;

pub fn cache_keys_step(
    catalog: &ToolCatalog,
    fetch_roots: &[String],
    tofu_roots: &[String],
) -> Result<Option<Step>, PlanSourcesError> {
    if fetch_roots.is_empty() && tofu_roots.is_empty() {
        return Ok(None);
    }
    let mut script = prelude();
    if !fetch_roots.is_empty() {
        CacheFamily::Sources
            .validate_key_bound()
            .map_err(PlanSourcesError::Contract)?;
        script.extend(sources_commands(fetch_roots)?);
    }
    if !tofu_roots.is_empty() {
        CacheFamily::TofuProviders
            .validate_key_bound()
            .map_err(PlanSourcesError::Contract)?;
        script.extend(tofu_commands(catalog, tofu_roots)?);
    }
    script.push(format!(
        "printf '%s\\n' \"{TRUST_OUTPUT}=$trust\" >> \"$GITHUB_OUTPUT\""
    ));
    let env = BTreeMap::from([(DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned())]);
    let mut step = crate::steps::shell_step(
        CACHE_KEYS_STEP_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script.join("; ")],
        env,
    )
    .map_err(PlanSourcesError::ShellRender)?;
    step.id = Some(StepId::CacheKeys);
    step.role = Some(StepRole::CacheKeys);
    Ok(Some(step))
}

fn prelude() -> Vec<String> {
    vec![
        "set -eu".to_owned(),
        "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac".to_owned(),
        "d=\"$RUNNER_TEMP/velnor/cache-keys\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        "s256() { sha256sum \"$1\" > \"$d/sum\"; h=; rest=; read -r h rest < \"$d/sum\" || [ -n \"$h\" ]; case \"$h\" in ''|*[!0-9a-f]*) printf '%s\\n' 'sha256sum returned no digest' >&2; exit 1 ;; esac; [ \"${#h}\" -eq 64 ] || exit 1; }".to_owned(),
        "uname -s > \"$d/platform\"".to_owned(),
        "uname -m >> \"$d/platform\"".to_owned(),
        format!("default_ref=\"${{{DEFAULT_REF_ENV}:-}}\""),
        "trust=none".to_owned(),
        "if [ \"${GITHUB_EVENT_NAME:-}\" = push ] && [ -n \"${default_ref#refs/heads/}\" ] && [ \"${GITHUB_REF:-}\" = \"$default_ref\" ] && [ \"${GITHUB_REF_PROTECTED:-}\" = true ]; then trust=trusted; fi".to_owned(),
        format!("check_key() {{ [ \"${{#1}}\" -le {MAX_KEY} ] || {{ printf 'cache key exceeds {MAX_KEY} bytes: %s\\n' \"$1\" >&2; exit 1; }}; }}"),
    ]
}

fn compat_header(family: CacheFamily, root_locator: Option<&str>, file: &str) -> Vec<String> {
    let spec = family.spec();
    let mut lines = vec![
        format!("family={}", spec.layer),
        format!("schema={}", spec.schema),
    ];
    lines.extend(spec.shell_paths(root_locator));
    let quoted = lines
        .iter()
        .map(|line| format!("\"{line}\""))
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        format!("printf '%s\\n' {quoted} > \"{file}\""),
        format!("cat \"$d/platform\" >> \"{file}\""),
    ]
}

fn sources_commands(fetch_roots: &[String]) -> Result<Vec<String>, PlanSourcesError> {
    let prefix = CacheFamily::Sources.key_prefix();
    let home = CARGO_HOME_EXPR.replace(RUNNER_TEMP_EXPR, RUNNER_TEMP_SHELL);
    let mut script = compat_header(CacheFamily::Sources, None, "$d/sources-compat");
    script.push(format!(
        "printf '%s\\n' \"{home}\" >> \"$d/sources-compat\""
    ));
    script.push("s256 \"$d/sources-compat\"".to_owned());
    script.push("compat=\"s256-$h\"".to_owned());
    script.push(": > \"$d/sources-locks\"".to_owned());
    for root in fetch_roots {
        validate_fetch_root(root).map_err(PlanSourcesError::Contract)?;
        let lock = workspace_lock(root);
        let label = if root.is_empty() { "." } else { root.as_str() };
        script.push(format!("s256 \"$GITHUB_WORKSPACE/{lock}\""));
        script.push(format!(
            "printf '%s\\t%s\\n' \"{label}\" \"$h\" >> \"$d/sources-locks\""
        ));
    }
    script.extend([
        "LC_ALL=C sort \"$d/sources-locks\" > \"$d/sources-snapshot\"".to_owned(),
        "s256 \"$d/sources-snapshot\"".to_owned(),
        "snap=\"s256-$h\"".to_owned(),
        format!("trusted=\"{prefix}trusted-$compat-\""),
        "key=\"$trusted$snap\"".to_owned(),
        "check_key \"$key\"".to_owned(),
        "save=false".to_owned(),
        "[ \"$trust\" = none ] || save=true".to_owned(),
        format!("printf '%s\\n' \"{SOURCES_KEY_OUTPUT}=$key\" \"{SOURCES_SAVE_OUTPUT}=$save\" \"{SOURCES_SNAPSHOT_OUTPUT}=$snap\" '{SOURCES_RESTORE_KEYS_OUTPUT}<<{OUTPUT_DELIMITER}' >> \"$GITHUB_OUTPUT\""),
        format!("printf '%s\\n' \"$trusted\" '{OUTPUT_DELIMITER}' >> \"$GITHUB_OUTPUT\""),
        "printf 'velnor: cache trust=%s sources key=%s save=%s\\n' \"$trust\" \"$key\" \"$save\"".to_owned(),
    ]);
    Ok(script)
}

fn tofu_commands(
    catalog: &ToolCatalog,
    tofu_roots: &[String],
) -> Result<Vec<String>, PlanSourcesError> {
    let prefix = CacheFamily::TofuProviders.key_prefix();
    let spec = catalog.tool_spec(PinnedTool::Opentofu);
    let mut script = vec![
        "mkdir -p \"$d/tofu-empty\"".to_owned(),
        format!(
            "(cd \"$d/tofu-empty\" && mise --no-config --no-env --no-hooks exec {spec} -- tofu version) > \"$d/tofu-version\""
        ),
    ];
    for (ordinal, root) in tofu_roots.iter().enumerate() {
        validate_fetch_root(root).map_err(PlanSourcesError::Contract)?;
        let locator =
            velnor_actions_tofu::tofu_root_locator(root).map_err(PlanSourcesError::Contract)?;
        let file = format!("$d/tofu-compat-{ordinal}");
        let lock = if root.is_empty() {
            ".terraform.lock.hcl".to_owned()
        } else {
            format!("{root}/.terraform.lock.hcl")
        };
        let output = tofu_providers_key_output(ordinal);
        script.extend(compat_header(
            CacheFamily::TofuProviders,
            Some(&locator),
            &file,
        ));
        script.extend([
            format!("cat \"$d/tofu-version\" >> \"{file}\""),
            format!("s256 \"{file}\""),
            "tc=\"s256-$h\"".to_owned(),
            format!("lock=\"$GITHUB_WORKSPACE/{lock}\""),
            "if [ -f \"$lock\" ]; then s256 \"$lock\"; else printf '%s\\n' absent > \"$d/tofu-absent\"; s256 \"$d/tofu-absent\"; fi".to_owned(),
            format!("tkey=\"{prefix}trusted-$tc-s256-$h\""),
            "check_key \"$tkey\"".to_owned(),
            format!("printf '%s\\n' \"{output}=$tkey\" >> \"$GITHUB_OUTPUT\""),
        ]);
    }
    Ok(script)
}

fn workspace_lock(root: &str) -> String {
    if root.is_empty() {
        "Cargo.lock".to_owned()
    } else {
        format!("{root}/Cargo.lock")
    }
}

#[cfg(test)]
mod tests {
    use super::{prelude, sources_commands};

    fn script() -> String {
        prelude().join("\n")
    }

    fn key_script() -> String {
        sources_commands(&[String::new()])
            .expect("key commands render")
            .join("\n")
    }

    #[test]
    fn cache_keys_never_trust_pull_requests() {
        let text = script();
        assert!(!text.contains("trust=pr"));
        assert!(!text.contains("pull_request"));
        let keys = key_script();
        assert!(!keys.contains("trust=pr"));
        assert!(!keys.contains("pull_request"));
        assert!(!keys.contains("-pr-"));
    }

    #[test]
    fn cache_keys_still_gate_trusted_push() {
        let text = script();
        assert!(text.contains("trust=none"));
        assert!(text.contains("then trust=trusted; fi"));
        assert!(text.contains("GITHUB_REF_PROTECTED"));
    }
}
