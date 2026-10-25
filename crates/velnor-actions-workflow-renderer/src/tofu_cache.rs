pub use velnor_actions_contract::workflow::step_identity::{
    TOFU_PROVIDER_ADMISSION_USES, TOFU_PROVIDER_CACHE_BASE_EXPR, TOFU_PROVIDERS_KEY_OUTPUT_EXPR,
    TOFU_PROVIDERS_PATH_OUTPUT_EXPR,
};
use velnor_actions_contract::{Step, StepRole};

use crate::{RenderError, marker, steps, yaml};

pub const TOFU_PROVIDERS_RESTORE_NAME: &str = "Restore Tofu providers";
pub const TOFU_PROVIDERS_SAVE_NAME: &str = "Save Tofu providers";
pub const TOFU_PROVIDERS_SAVE_USES: &str =
    "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
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

pub(crate) fn provider_admission_file(
    version: &str,
) -> Result<crate::tree::RenderedFile, RenderError> {
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
    let bytes = marker::with_marker(version, &yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(crate::tree::RenderedFile {
        path: ".github/actions/tofu-provider-admission/action.yml".to_owned(),
        bytes,
    })
}

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

fn rename_step(mut step: Step, name: &str) -> Result<Step, RenderError> {
    crate::steps::scan_for_private_subcommands(name)?;
    name.clone_into(&mut step.name);
    Ok(step)
}

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
