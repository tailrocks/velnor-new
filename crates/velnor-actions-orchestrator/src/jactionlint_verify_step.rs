//! `Verify jactionlint`: Velnor's own check of the installed linter bytes.
//!
//! Mise's `github:jdx/jactionlint` backend already checks the release-API
//! digest and the GitHub artifact attestation, which trust GitHub and the
//! upstream release workflow. This step adds the Velnor-pinned control,
//! modelled on `Verify Rust channel manifest`: it resolves the installed
//! `jactionlint` executable through `mise which` and compares its SHA-256
//! with [`velnor_actions_mise::JACTIONLINT_DIGESTS`] for the runner platform.
//! The catalog pin must have a digest; a project-declared version without
//! one (hard decision 4) keeps Mise's attestation check and gets a notice.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_mise::{JACTIONLINT_DIGESTS, JACTIONLINT_VERSION, PinnedTool, ToolCatalog};

use crate::OrchestratorError;

/// Contract-fixed display name of the executable verification step.
pub(crate) const VERIFY_JACTIONLINT_STEP: &str = "Verify jactionlint";

/// Stable failure code for a catalog pin without an executable digest.
pub(crate) const NOT_CATALOGUED_CODE: &str = "jactionlint_executable_not_catalogued";

/// Fixed one-line POSIX script; `$(`, backticks and newlines are rejected
/// by the step validator, so the path is read from a file and digests
/// compare through `sha256sum -c`.
pub(crate) fn script(catalog: &ToolCatalog) -> String {
    let version = catalog.version(PinnedTool::Jactionlint);
    let cases = JACTIONLINT_DIGESTS
        .iter()
        .map(|row| {
            [
                row.version,
                "/",
                row.platform,
                ") want=",
                row.executable_sha256,
                ";; ",
            ]
            .concat()
        })
        .collect::<String>();
    let fail = "printf \"%s\\n\" \"::error::";
    [
        "set -eu".to_owned(),
        format!("v=\"{version}\""),
        "case \"${RUNNER_OS-}-${RUNNER_ARCH-}\" in Linux-X64) p=linux-x64;; Linux-ARM64) p=linux-arm64;; macOS-ARM64) p=macos-arm64;; macOS-X64) p=macos-x64;; *) p=unsupported;; esac".to_owned(),
        format!("case \"$v/$p\" in {cases}*) want=;; esac"),
        format!(
            "if [ -z \"$want\" ]; then if [ \"$v\" = \"{JACTIONLINT_VERSION}\" ]; then {fail}{NOT_CATALOGUED_CODE}: jactionlint $v on $p has no executable digest in this Velnor catalog\" >&2; exit 1; fi; printf \"%s\\n\" \"::notice::jactionlint $v on $p has no Velnor executable digest; the Mise release digest and GitHub attestation checks still applied\"; exit 0; fi"
        ),
        "d=\"$RUNNER_TEMP/velnor/jactionlint-verify\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        format!(
            "mise --no-config --no-env --no-hooks which --tool \"jactionlint@$v\" jactionlint > \"$d/path\" || {{ {fail}jactionlint_not_installed: jactionlint@$v\" >&2; exit 1; }}"
        ),
        "IFS= read -r exe < \"$d/path\"".to_owned(),
        format!("test -f \"$exe\" || {{ {fail}jactionlint_executable_missing: $exe\" >&2; exit 1; }}"),
        "printf \"%s  %s\\n\" \"$want\" \"$exe\" > \"$d/executable.sha256\"".to_owned(),
        format!(
            "if command -v sha256sum >/dev/null 2>&1; then sha256sum -c \"$d/executable.sha256\" >/dev/null; else shasum -a 256 -c \"$d/executable.sha256\" >/dev/null; fi || {{ {fail}jactionlint_executable_digest_mismatch: $exe is not SHA-256 $want\" >&2; exit 1; }}"
        ),
        "printf \"%s\\n\" \"velnor: jactionlint $v ($p) executable SHA-256 $want verified\"".to_owned(),
    ]
    .join("; ")
}

/// `sh -c` argv for the verification script (release-gate steps).
pub(crate) fn argv(catalog: &ToolCatalog) -> Vec<String> {
    vec!["sh".to_owned(), "-c".to_owned(), script(catalog)]
}

/// Typed, credential-scrubbed verification step after `Prepare pinned tools`.
///
/// # Errors
///
/// Returns a contract error when the renderer rejects the fixed argv.
pub(crate) fn verify_jactionlint_step(catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    Ok(velnor_actions_workflow_renderer::steps::shell_step(
        VERIFY_JACTIONLINT_STEP,
        argv(catalog),
        BTreeMap::new(),
    )?)
}

#[cfg(test)]
#[path = "jactionlint_verify_step_tests.rs"]
mod tests;
