//! Check the Rust channel manifest against the catalog digest.
//!
//! Mise installs `core:rust` through rustup and writes no checksum in `mise.lock`.
//! Rustup checks downloaded components against SHA-256 values in the channel manifest.
//! This step downloads `channel-rust-<version>.toml` from static.rust-lang.org.
//! It checks that file against [`velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS`].
//! Each hash recorded in `lib/rustlib/multirust-channel-manifest.toml` must appear in the verified manifest.
//! This check compares manifest metadata. It does not hash installed files.
//! A version without a catalog digest fails the job.
//!
//! Read `${VELNOR_TOOL_RUST}` at runtime and include the whole digest table.
//! Do not copy the project version into the generated text (hard decision 4).

use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS;
use velnor_actions_mise::{PinnedTool, ToolCatalog, ToolHomes};

use crate::OrchestratorError;

/// Contract-fixed display name of the manifest verification step.
pub(crate) const VERIFY_RUST_MANIFEST_STEP: &str = "Verify Rust channel manifest";

/// Stable failure code for a Rust version without a catalog digest.
pub(crate) const NOT_CATALOGUED_CODE: &str = "rust_channel_manifest_not_catalogued";

/// Official channel-manifest URL prefix (versioned archive, never `stable`).
const DIST: &str = "https://static.rust-lang.org/dist/channel-rust-";

/// Use one line of POSIX shell script.
/// The step validator rejects `$(`, backticks, and newlines.
/// Compare digests with `sha256sum -c`.
fn script(catalog: &ToolCatalog) -> String {
    let version = catalog.version(PinnedTool::Rust);
    let toolchain = catalog.rust_toolchain_name();
    let cases = RUST_CHANNEL_MANIFESTS
        .iter()
        .map(|(known, digest, _)| [*known, ") want=", *digest, ";; "].concat())
        .collect::<String>();
    let fail = "printf \"%s\\n\" \"::error::";
    [
        "set -eu".to_owned(),
        format!("v=\"{version}\""),
        format!(
            "case \"$v\" in {cases}*) {fail}{NOT_CATALOGUED_CODE}: Rust $v has no channel manifest digest in this Velnor catalog. Upgrade velnor-actions or pin a Rust version in the catalog.\" >&2; exit 1;; esac"
        ),
        "d=\"$RUNNER_TEMP/velnor/rust-manifest\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        format!(
            "curl --proto \"=https\" --proto-redir \"=https\" --tlsv1.2 --fail --silent --show-error --location --retry 3 --output \"$d/channel.toml\" \"{DIST}$v.toml\""
        ),
        "printf \"%s  %s\\n\" \"$want\" \"$d/channel.toml\" > \"$d/channel.sha256\"".to_owned(),
        format!(
            "if command -v sha256sum >/dev/null 2>&1; then sha256sum -c \"$d/channel.sha256\" >/dev/null; else shasum -a 256 -c \"$d/channel.sha256\" >/dev/null; fi || {{ {fail}rust_channel_manifest_digest_mismatch: The SHA-256 digest of channel-rust-$v.toml does not match $want.\" >&2; exit 1; }}"
        ),
        format!(
            "m=\"$RUSTUP_HOME/toolchains/{toolchain}/lib/rustlib/multirust-channel-manifest.toml\""
        ),
        format!(
            "test -f \"$m\" || {{ {fail}rust_installed_manifest_missing: $m\" >&2; exit 1; }}"
        ),
        format!(
            "if grep -Eo \"[0-9a-f]{{64}}\" \"$d/channel.toml\" > \"$d/pinned.raw\"; then :; else status=$?; if [ \"$status\" -gt 1 ]; then {fail}rust_channel_manifest_read_failed: The verifier could not read the downloaded channel manifest.\" >&2; exit 1; fi; fi"
        ),
        "LC_ALL=C sort -u \"$d/pinned.raw\" > \"$d/pinned\"".to_owned(),
        format!(
            "if grep -Eo \"[0-9a-f]{{64}}\" \"$m\" > \"$d/installed.raw\"; then :; else status=$?; if [ \"$status\" -gt 1 ]; then {fail}rust_installed_manifest_read_failed: The verifier could not read the installed toolchain manifest.\" >&2; exit 1; fi; fi"
        ),
        "LC_ALL=C sort -u \"$d/installed.raw\" > \"$d/installed\"".to_owned(),
        format!(
            "test -s \"$d/installed\" || {{ {fail}rust_installed_manifest_empty: $m records no hashes.\" >&2; exit 1; }}"
        ),
        "LC_ALL=C comm -23 \"$d/installed\" \"$d/pinned\" > \"$d/unpinned\"".to_owned(),
        format!(
            "if test -s \"$d/unpinned\"; then {fail}rust_component_hash_not_in_manifest: The installed toolchain manifest records hashes absent from the verified channel-rust-$v.toml.\" >&2; exit 1; fi"
        ),
        "printf \"%s\\n\" \"velnor: The Rust $v channel manifest matches SHA-256 $want. It contains every hash recorded in the installed manifest.\"".to_owned(),
    ]
    .join("; ")
}

/// Typed, credential-scrubbed verification step for the job's Rust install.
///
/// # Errors
///
/// Returns a contract error when the renderer rejects the fixed argv.
pub(crate) fn verify_rust_manifest_step(catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    let homes = ToolHomes::runner_temp();
    let env = BTreeMap::from([("RUSTUP_HOME".to_owned(), homes.rustup_home().to_owned())]);
    Ok(velnor_actions_workflow_renderer::steps::shell_step(
        VERIFY_RUST_MANIFEST_STEP,
        vec!["sh".to_owned(), "-c".to_owned(), script(catalog)],
        env,
    )?)
}

#[cfg(test)]
#[path = "rust_manifest_step_tests.rs"]
mod tests;
