//! Tofu provider-cache step templates: saves plus the owned-path allowlist.
//!
//! Restore steps arrive from the orchestrator (per-root keys need the
//! catalog tofu pin); saves append post-hoc through writer election
//! ([`elect_tofu_provider_savers`](crate::cache_p08::elect_tofu_provider_savers)).
//! Both archive exactly one job-private plugin-cache dir, never the
//! data dir beside it.

pub use velnor_actions_contract::workflow::step_identity::{
    TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDER_CACHE_BASE_EXPR, TOFU_PROVIDERS_KEY_PREFIX,
};
use velnor_actions_contract::{Step, StepRole};

use crate::{RenderError, cache_steps, marker, steps, yaml};

/// Display name of the provider restore step.
pub const TOFU_PROVIDERS_RESTORE_NAME: &str = "Restore Tofu providers";
/// Display name of the provider save step.
pub const TOFU_PROVIDERS_SAVE_NAME: &str = "Save Tofu providers";
/// Pinned `actions/cache/save` ref (v6.1.0, qualified 2026-09-28).
///
/// Same qualified pin as the tools save; a pin bump moves both.
pub const TOFU_PROVIDERS_SAVE_USES: &str =
    "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Admission shell shared by all provider-cache steps.
pub const TOFU_PROVIDER_ADMISSION_SCRIPT: &str = r#"set -eu; cd -P "$RUNNER_TEMP"; rt=$PWD; slug="$TOFU_PROVIDER_CACHE_SLUG"; case "$slug" in ''|*[!A-Za-z0-9_-]*) exit 1 ;; esac; v="$rt/velnor"; p="$v/tofu-cache"; [ ! -L "$v" ] && [ ! -L "$p" ] || exit 1; mkdir -p "$p"; cd -P "$p"; [ "$PWD" = "$p" ] || exit 1; d="$PWD/$slug"; if [ "$TOFU_CACHE_HIT" = true ] && [ -n "$TOFU_EXPECTED_KEY" ] && [ "$TOFU_MATCHED_KEY" = "$TOFU_EXPECTED_KEY" ] && [ -d "$d" ] && [ ! -L "$d" ]; then exit 0; fi; rm -rf "$d"; mkdir -m 700 "$d""#;

/// Render the single local composite that admits provider-cache restore outputs.
/// # Errors
pub(crate) fn provider_admission_file(
    version: &str,
) -> Result<crate::tree::RenderedFile, RenderError> {
    let inputs = ["cache-hit", "expected-key", "matched-key", "cache-slug"]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                yaml::Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        yaml::Yaml::str(format!("Provider cache {name}")),
                    ),
                    ("required".to_owned(), yaml::Yaml::Bool(true)),
                ]),
            )
        })
        .collect();
    let env = [
        ("TOFU_CACHE_HIT", "cache-hit"),
        ("TOFU_EXPECTED_KEY", "expected-key"),
        ("TOFU_MATCHED_KEY", "matched-key"),
        ("TOFU_PROVIDER_CACHE_SLUG", "cache-slug"),
    ]
    .into_iter()
    .map(|(key, input)| {
        (
            key.to_owned(),
            yaml::Yaml::str(format!("${{{{ inputs.{input} }}}}")),
        )
    })
    .collect();
    let body = yaml::Yaml::Map(vec![
        (
            "name".to_owned(),
            yaml::Yaml::str("Admit OpenTofu provider cache"),
        ),
        (
            "description".to_owned(),
            yaml::Yaml::str("Discard provider cache bytes unless the exact restore key matched."),
        ),
        ("inputs".to_owned(), yaml::Yaml::Map(inputs)),
        (
            "runs".to_owned(),
            yaml::Yaml::Map(vec![
                ("using".to_owned(), yaml::Yaml::str("composite")),
                (
                    "steps".to_owned(),
                    yaml::Yaml::Seq(vec![yaml::Yaml::Map(vec![
                        ("shell".to_owned(), yaml::Yaml::str("bash")),
                        ("env".to_owned(), yaml::Yaml::Map(env)),
                        (
                            "run".to_owned(),
                            yaml::Yaml::str(TOFU_PROVIDER_ADMISSION_SCRIPT),
                        ),
                    ])]),
                ),
            ]),
        ),
    ]);
    let quoted = yaml::quote_run_values_in_yaml(body);
    let bytes = marker::with_marker(version, &yaml::render_yaml(&quoted))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(crate::tree::RenderedFile {
        path: ".github/actions/tofu-provider-admission/action.yml".to_owned(),
        bytes,
    })
}

/// Provider save step over the pinned save action.
///
/// The key and path read back from the elected restore; the push
/// gate arrives from the election caller, never here.
/// # Errors
pub fn tofu_providers_save_step(key: &str, path: &str) -> Result<Step, RenderError> {
    let step = cache_steps::cache_action_step(
        false,
        TOFU_PROVIDERS_SAVE_USES,
        "tofu-providers",
        key,
        &[],
        &[path.to_owned()],
    )?;
    let mut step = rename_step(step, TOFU_PROVIDERS_SAVE_NAME)?;
    step.role = Some(StepRole::TofuProvidersSave);
    Ok(step)
}

/// Rename a built step; names are fixed by the caller contract.
fn rename_step(mut step: Step, name: &str) -> Result<Step, RenderError> {
    crate::steps::scan_for_private_subcommands(name)?;
    name.clone_into(&mut step.name);
    Ok(step)
}

/// True for exactly one owned plugin-cache dir under the base.
///
/// Single path segment, slug charset only, no traversal, no
/// never-archive names (state, plans, credentials), never the bare
/// base: the data dir beside it (`tofu-data`) and every foreign tree
/// stay out.
#[must_use]
pub fn tofu_providers_path_ok(path: &str) -> bool {
    if path.contains("..") || crate::cache_steps::is_never_archive_path(path) {
        return false;
    }
    let Some(slug) = path.strip_prefix(&format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/")) else {
        return false;
    };
    !slug.is_empty()
        && !slug.contains('/')
        && slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
