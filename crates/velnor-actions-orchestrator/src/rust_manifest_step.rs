//! `Verify Rust channel manifest`: closes `core:rust`'s `no_checksums` gap.
//!
//! Mise's `core:rust` backend installs through rustup and writes no
//! checksum into `mise.lock`. rustup verifies every component against the
//! SHA-256 values in the channel manifest it downloads. This step pins that
//! manifest: it fetches `channel-rust-<version>.toml` from
//! static.rust-lang.org, checks it against the catalog digest
//! ([`velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS`]), and requires
//! every hash recorded in the installed toolchain's
//! `lib/rustlib/multirust-channel-manifest.toml` to appear in the verified
//! manifest. A version the catalog does not know fails the job.
//!
//! The generated text stays version-free (hard decision 4): the step reads
//! the run-time `${VELNOR_TOOL_RUST}` and carries the whole digest table.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepRole};
use velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS;
use velnor_actions_mise::{PinnedTool, ToolCatalog, ToolHomes};

use crate::OrchestratorError;

/// Contract-fixed display name of the manifest verification step.
pub(crate) const VERIFY_RUST_MANIFEST_STEP: &str = "Verify Rust channel manifest";

/// Stable failure code for a Rust version without a catalog digest.
pub(crate) const NOT_CATALOGUED_CODE: &str = "rust_channel_manifest_not_catalogued";

/// Official channel-manifest URL prefix (versioned archive, never `stable`).
const DIST: &str = "https://static.rust-lang.org/dist/channel-rust-";

/// Fixed one-line POSIX script; `$(`, backticks and newlines are rejected
/// by the step validator, so digests compare through `sha256sum -c`.
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
            "case \"$v\" in {cases}*) {fail}{NOT_CATALOGUED_CODE}: Rust $v has no channel-manifest digest in this Velnor catalog; upgrade velnor-actions or pin a catalogued Rust\" >&2; exit 1;; esac"
        ),
        "d=\"$RUNNER_TEMP/velnor/rust-manifest\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        format!(
            "curl --proto \"=https\" --proto-redir \"=https\" --tlsv1.2 --fail --silent --show-error --location --retry 3 --output \"$d/channel.toml\" \"{DIST}$v.toml\""
        ),
        "printf \"%s  %s\\n\" \"$want\" \"$d/channel.toml\" > \"$d/channel.sha256\"".to_owned(),
        format!(
            "if command -v sha256sum >/dev/null 2>&1; then sha256sum -c \"$d/channel.sha256\" >/dev/null; else shasum -a 256 -c \"$d/channel.sha256\" >/dev/null; fi || {{ {fail}rust_channel_manifest_digest_mismatch: channel-rust-$v.toml is not SHA-256 $want\" >&2; exit 1; }}"
        ),
        format!(
            "m=\"$RUSTUP_HOME/toolchains/{toolchain}/lib/rustlib/multirust-channel-manifest.toml\""
        ),
        format!(
            "test -f \"$m\" || {{ {fail}rust_installed_manifest_missing: $m\" >&2; exit 1; }}"
        ),
        "grep -Eo \"[0-9a-f]{64}\" \"$d/channel.toml\" | LC_ALL=C sort -u > \"$d/pinned\"".to_owned(),
        "grep -Eo \"[0-9a-f]{64}\" \"$m\" | LC_ALL=C sort -u > \"$d/installed\"".to_owned(),
        format!(
            "test -s \"$d/installed\" || {{ {fail}rust_installed_manifest_empty: $m records no hashes\" >&2; exit 1; }}"
        ),
        "LC_ALL=C comm -23 \"$d/installed\" \"$d/pinned\" > \"$d/unpinned\"".to_owned(),
        format!(
            "if test -s \"$d/unpinned\"; then {fail}rust_component_hash_not_in_manifest: the installed toolchain records hashes absent from the verified channel-rust-$v.toml\" >&2; exit 1; fi"
        ),
        "printf \"%s\\n\" \"velnor: Rust $v channel manifest SHA-256 $want verified; every installed component hash is in it\"".to_owned(),
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
    let mut step = velnor_actions_workflow_renderer::steps::shell_step(
        VERIFY_RUST_MANIFEST_STEP,
        vec!["sh".to_owned(), "-c".to_owned(), script(catalog)],
        env,
    )?;
    step.role = Some(StepRole::VerifyRustManifest);
    Ok(step)
}

#[cfg(test)]
#[path = "rust_manifest_step_tests.rs"]
mod tests;
