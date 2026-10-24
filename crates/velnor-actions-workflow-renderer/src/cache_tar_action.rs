//! One generated composite for installing the verified cache-action aliases.

use std::collections::BTreeMap;

use crate::{RenderError, commands, marker, steps, tree::RenderedFile, yaml::Yaml};

const ACTION_OUTPUT_PATH: &str = ".github/actions/velnor-cache-tar-launchers/action.yml";

pub(crate) fn action_file(
    staged_binary: &str,
    helper_version: &str,
    marker_version: &str,
) -> Result<RenderedFile, RenderError> {
    crate::guard::validate_staged_binary(staged_binary, helper_version)?;
    let helper_sha256 = format!(
        "${{{{ inputs.{} }}}}",
        steps::CACHE_TAR_LAUNCHER_SHA256_INPUT
    );
    let env = BTreeMap::from([
        (
            steps::INTERNAL_OP_ENV.to_owned(),
            steps::CACHE_TAR_LAUNCHER_INSTALL_OPERATION.to_owned(),
        ),
        (
            steps::CACHE_TAR_VERSION_ENV.to_owned(),
            helper_version.to_owned(),
        ),
        (
            steps::CACHE_TAR_SHA256_ENV.to_owned(),
            helper_sha256.clone(),
        ),
    ]);
    commands::validate_composite_env(&env)?;
    let run = commands::quote_run_arg(staged_binary)?;
    let body = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Velnor cache tar adapters")),
        (
            "description".to_owned(),
            Yaml::str("Install the tar entrypoints used by cache action steps."),
        ),
        (
            "inputs".to_owned(),
            Yaml::Map(vec![(
                steps::CACHE_TAR_LAUNCHER_SHA256_INPUT.to_owned(),
                Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        Yaml::str("SHA-256 hash for the helper file used by this action."),
                    ),
                    ("required".to_owned(), Yaml::Bool(true)),
                ]),
            )]),
        ),
        (
            "outputs".to_owned(),
            Yaml::Map(vec![(
                "path".to_owned(),
                Yaml::Map(vec![
                    (
                        "description".to_owned(),
                        Yaml::str(
                            "The `PATH` value with cache aliases first, followed by the inherited value.",
                        ),
                    ),
                    (
                        "value".to_owned(),
                        Yaml::str("${{ steps.install.outputs.path }}"),
                    ),
                ]),
            )]),
        ),
        (
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite")),
                (
                    "steps".to_owned(),
                    Yaml::Seq(vec![composite_install_step(&env, run)]),
                ),
            ]),
        ),
    ]);
    let bytes = marker::with_marker(marker_version, &crate::yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: ACTION_OUTPUT_PATH.to_owned(),
        bytes,
    })
}

fn composite_install_step(env: &BTreeMap<String, String>, run: String) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Install cache tar adapters")),
        ("id".to_owned(), Yaml::str("install")),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            super::document_steps::string_map_yaml(env),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

#[cfg(test)]
#[path = "cache_tar_action_tests.rs"]
mod tests;
