//! Tofu provider-cache step templates: saves plus the owned-path allowlist.
//!
//! Restore steps arrive from the orchestrator (per-root keys need the
//! catalog tofu pin); saves append post-hoc through writer election
//! ([`elect_tofu_provider_savers`](crate::cache_p08::elect_tofu_provider_savers)).
//! Both archive exactly one job-private plugin-cache dir, never the
//! data dir beside it.

pub use velnor_actions_contract_workflow::workflow::step_identity::{
    TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDER_CACHE_BASE_EXPR, TOFU_PROVIDERS_KEY_OUTPUT_EXPR,
    TOFU_PROVIDERS_KEY_PREFIX, TOFU_PROVIDERS_PATH_OUTPUT_EXPR,
};
use velnor_actions_contract_workflow::{Step, StepRole};

use velnor_actions_workflow_steps::{RenderError, steps};
use velnor_actions_workflow_tree::{marker, yaml};

/// Display name of the provider restore step.
pub const TOFU_PROVIDERS_RESTORE_NAME: &str = "Restore Tofu providers";
/// Display name of the provider save step.
pub const TOFU_PROVIDERS_SAVE_NAME: &str = "Save Tofu providers";
/// Pinned `actions/cache/save` ref (v6.1.0, qualified 2026-09-28).
///
/// Same qualified pin as the tools save; a pin bump moves both.
pub const TOFU_PROVIDERS_SAVE_USES: &str =
    "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Admission shell shared by every provider-cache composite invocation.
pub const TOFU_PROVIDER_ADMISSION_SCRIPT: &str = r#"set -eu
cd -P "$RUNNER_TEMP"
rt=$PWD
v="$rt/velnor"
p="$v/tofu-cache"
[ ! -L "$v" ] && [ ! -L "$p" ] || exit 1
mkdir -p "$p"
cd -P "$p"
[ "$PWD" = "$p" ] || exit 1
prefix="$p/"
case "$TOFU_PROVIDER_CACHE_PATH" in
    "$prefix"*) slug=${TOFU_PROVIDER_CACHE_PATH#"$prefix"} ;;
    *) exit 1 ;;
esac
case "$slug" in
    ''|*/*|*[!A-Za-z0-9_-]*) exit 1 ;;
esac
d="$PWD/$slug"
[ "$TOFU_PROVIDER_CACHE_PATH" = "$d" ] || exit 1
if [ "$TOFU_CACHE_HIT" = true ] \
    && [ -n "$TOFU_EXPECTED_KEY" ] \
    && [ "$TOFU_MATCHED_KEY" = "$TOFU_EXPECTED_KEY" ] \
    && [ -d "$d" ] \
    && [ ! -L "$d" ]; then
    exit 0
fi
rm -rf "$d"
mkdir -m 700 "$d""#;

/// Render the single local composite that restores and admits provider-cache bytes.
/// # Errors
pub(crate) fn provider_admission_file(
    version: &str,
) -> Result<velnor_actions_workflow_tree::rendered::RenderedFile, RenderError> {
    let body = yaml::Yaml::Map(vec![
        (
            "name".to_owned(),
            yaml::Yaml::str("Admit OpenTofu provider cache"),
        ),
        (
            "description".to_owned(),
            yaml::Yaml::str("Restore and admit the exact OpenTofu provider cache entry."),
        ),
        ("inputs".to_owned(), provider_composite_inputs()),
        ("outputs".to_owned(), provider_composite_outputs()),
        (
            "runs".to_owned(),
            yaml::Yaml::Map(vec![
                ("using".to_owned(), yaml::Yaml::str("composite")),
                (
                    "steps".to_owned(),
                    yaml::Yaml::Seq(vec![provider_restore_action()?, provider_admission_step()]),
                ),
            ]),
        ),
    ]);
    let quoted = yaml::quote_run_values_in_yaml(body);
    let bytes = marker::with_marker(version, &yaml::render_yaml(&quoted))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(velnor_actions_workflow_tree::rendered::RenderedFile {
        path: ".github/actions/tofu-provider-admission/action.yml".to_owned(),
        bytes,
    })
}

/// Required key and path inputs for the provider composite.
fn provider_composite_inputs() -> yaml::Yaml {
    yaml::Yaml::Map(
        ["cache-key", "cache-path"]
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
            .collect(),
    )
}

/// Save-owned values exposed for an elected cache writer.
fn provider_composite_outputs() -> yaml::Yaml {
    yaml::Yaml::Map(
        [
            ("cache-key", "${{ inputs.cache-key }}"),
            ("cache-path", "${{ inputs.cache-path }}"),
        ]
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_owned(),
                yaml::Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        yaml::Yaml::str(format!("Provider cache {name}")),
                    ),
                    ("value".to_owned(), yaml::Yaml::str(value)),
                ]),
            )
        })
        .collect(),
    )
}

/// Exact restore call whose outputs feed the immediately following admission.
fn provider_restore_action() -> Result<yaml::Yaml, RenderError> {
    let restore_uses = TOFU_PROVIDERS_SAVE_USES
        .strip_prefix("actions/cache/save@")
        .map(|sha| format!("actions/cache/restore@{sha}"))
        .ok_or_else(|| RenderError::BadActionRef("bad_tofu_cache_save_pin".to_owned()))?;
    steps::validate_uses(&restore_uses)?;
    Ok(yaml::Yaml::Map(vec![
        ("name".to_owned(), yaml::Yaml::str("Restore Tofu providers")),
        ("id".to_owned(), yaml::Yaml::str("restore")),
        ("uses".to_owned(), yaml::Yaml::str(restore_uses)),
        (
            "with".to_owned(),
            yaml::Yaml::Map(vec![
                ("key".to_owned(), yaml::Yaml::str("${{ inputs.cache-key }}")),
                (
                    "path".to_owned(),
                    yaml::Yaml::str("${{ inputs.cache-path }}"),
                ),
            ]),
        ),
    ]))
}

/// Admit only an exact-key hit; all mismatches cold-start in the owned leaf.
fn provider_admission_step() -> yaml::Yaml {
    let env = [
        ("TOFU_CACHE_HIT", "steps.restore.outputs.cache-hit"),
        ("TOFU_EXPECTED_KEY", "inputs.cache-key"),
        (
            "TOFU_MATCHED_KEY",
            "steps.restore.outputs.cache-matched-key",
        ),
        ("TOFU_PROVIDER_CACHE_PATH", "inputs.cache-path"),
    ]
    .into_iter()
    .map(|(key, value)| {
        (
            key.to_owned(),
            yaml::Yaml::str(format!("${{{{ {value} }}}}")),
        )
    })
    .collect();
    yaml::Yaml::Map(vec![
        (
            "name".to_owned(),
            yaml::Yaml::str("Discard provider bytes unless the exact restore key matched"),
        ),
        ("shell".to_owned(), yaml::Yaml::str("bash")),
        ("env".to_owned(), yaml::Yaml::Map(env)),
        (
            "run".to_owned(),
            yaml::Yaml::str(TOFU_PROVIDER_ADMISSION_SCRIPT),
        ),
    ])
}

/// Provider save step over the cache identity emitted by the restore composite.
///
/// The exact key and path pass through from that job's composite step;
/// the push gate arrives from the election caller, never here.
/// # Errors
pub fn tofu_providers_save_step() -> Result<Step, RenderError> {
    let step = steps::action_step(
        TOFU_PROVIDERS_SAVE_NAME,
        TOFU_PROVIDERS_SAVE_USES,
        std::collections::BTreeMap::from([
            ("key".to_owned(), TOFU_PROVIDERS_KEY_OUTPUT_EXPR.to_owned()),
            (
                "path".to_owned(),
                TOFU_PROVIDERS_PATH_OUTPUT_EXPR.to_owned(),
            ),
        ]),
    )?;
    let mut step = rename_step(step, TOFU_PROVIDERS_SAVE_NAME)?;
    step.role = Some(StepRole::TofuProvidersSave);
    Ok(step)
}

/// Rename a built step; names are fixed by the caller contract.
fn rename_step(mut step: Step, name: &str) -> Result<Step, RenderError> {
    velnor_actions_workflow_steps::steps::scan_for_private_subcommands(name)?;
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

#[cfg(test)]
mod tests;
