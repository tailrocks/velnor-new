//! Canonical Jactionlint release facts and verifier script.
//!
//! The contract crate owns these immutable facts so leaf adapters can produce
//! and validate the same verifier without depending on the Mise adapter.

use super::selected_mise::BIND_SELECTED_MISE_FUNCTION;

/// Qualified Jactionlint release (`jdx/jactionlint` tag `v2.3.0`, stable,
/// published 2026-10-21). Install: Mise registry name `jactionlint`
/// (backend `github:jdx/jactionlint`), which checks the release-API digest
/// and the GitHub artifact attestation. Provenance was verified at this bump
/// (2026-10-22), never at generate time: `cosign verify-blob-attestation`
/// on `packslip.sigstore.json` (`--type https://packslip.dev/release/v1`,
/// identity `https://github.com/jdx/jactionlint/.github/workflows/release.yaml@refs/tags/v2.3.0`,
/// issuer `https://token.actions.githubusercontent.com`) and
/// `gh attestation verify -R jdx/jactionlint --signer-workflow jdx/jactionlint/.github/workflows/release.yaml`
/// both passed for the linux amd64, linux arm64, darwin arm64 and darwin
/// amd64 archives.
/// Per-platform digests: [`JACTIONLINT_DIGESTS`].
/// Source: `https://api.github.com/repos/jdx/jactionlint/releases/latest`; checked 2026-10-22.
pub const JACTIONLINT_VERSION: &str = "2.3.0";

/// One measured `jactionlint` release asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JactionlintDigest {
    /// Exact jactionlint version.
    pub version: &'static str,
    /// Mise platform (`linux-x64`, `linux-arm64`, `macos-arm64`, `macos-x64`).
    pub platform: &'static str,
    /// Upstream release asset name.
    pub asset: &'static str,
    /// SHA-256 of the release archive.
    pub archive_sha256: &'static str,
    /// SHA-256 of the `jactionlint` executable inside the archive.
    pub executable_sha256: &'static str,
}

/// Measured digests, sorted by version then platform.
pub const JACTIONLINT_DIGESTS: &[JactionlintDigest] = &[
    JactionlintDigest {
        version: "2.3.0",
        platform: "linux-arm64",
        asset: "jactionlint_2.3.0_linux_arm64.tar.gz",
        archive_sha256: "deea7744e5769e1e5cc8c9898a838f80a941ec68476d9f2814a61474d5652dac",
        executable_sha256: "7b6c395e8fb7c407d7b3358da1deab23434a75840dc76768ddb30119ce1041c0",
    },
    JactionlintDigest {
        version: "2.3.0",
        platform: "linux-x64",
        asset: "jactionlint_2.3.0_linux_amd64.tar.gz",
        archive_sha256: "907e6f6a59e181879f7e6936e19ee2eca1cbc51de134fb85460cfa8d65a71f28",
        executable_sha256: "3566633b33780130c5bea5342ad8c5505092e40094c7d6e25ee74e382899a5c7",
    },
    JactionlintDigest {
        version: "2.3.0",
        platform: "macos-arm64",
        asset: "jactionlint_2.3.0_darwin_arm64.tar.gz",
        archive_sha256: "f96afa36578b21290203ae8df4fa7ac3bf5aba08ed288df28cb0530af7ca6e55",
        executable_sha256: "b6853a909613dc9381548b1d61d278e1c4b1ab8334212f67ea6efa46f1f5d34b",
    },
    JactionlintDigest {
        version: "2.3.0",
        platform: "macos-x64",
        asset: "jactionlint_2.3.0_darwin_amd64.tar.gz",
        archive_sha256: "a61006b669ec0ab1266c73e3561eecf839537eb6e8b3b6be766a55ba17fcbe42",
        executable_sha256: "1aa284dc8b12ebd6bee858dcbbfb94c0b92e457cd7e4bdaee07ae2b9238dba45",
    },
];

fn row(version: &str, platform: &str) -> Option<&'static JactionlintDigest> {
    JACTIONLINT_DIGESTS
        .iter()
        .find(|row| row.version == version && row.platform == platform)
}

/// Pinned archive SHA-256 for one exact version on one Mise platform.
#[must_use]
pub fn jactionlint_archive_sha256(version: &str, platform: &str) -> Option<&'static str> {
    row(version, platform).map(|row| row.archive_sha256)
}

/// Pinned executable SHA-256 for one exact version on one Mise platform.
#[must_use]
pub fn jactionlint_executable_sha256(version: &str, platform: &str) -> Option<&'static str> {
    row(version, platform).map(|row| row.executable_sha256)
}

/// Build the exact POSIX Jactionlint verification script for one version.
///
/// The caller supplies the fixed system PATH used by the renderer. Project
/// references stay symbolic; literal versions are validated by the renderer.
#[must_use]
pub fn jactionlint_verifier_script(version: &str, verify_path: &str) -> String {
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
        format!("PATH=\"{verify_path}\"; export PATH"),
        format!("v=\"{version}\""),
        "case \"${RUNNER_OS-}-${RUNNER_ARCH-}\" in Linux-X64) p=linux-x64;; Linux-ARM64) p=linux-arm64;; macOS-ARM64) p=macos-arm64;; macOS-X64) p=macos-x64;; *) p=unsupported;; esac".to_owned(),
        format!("case \"$v/$p\" in {cases}*) want=;; esac"),
        format!(
            "if [ -z \"$want\" ]; then if [ \"$v\" = \"{JACTIONLINT_VERSION}\" ]; then {fail}jactionlint_executable_not_catalogued: jactionlint $v on $p has no executable digest in this Velnor catalog\" >&2; exit 1; fi; printf \"%s\\n\" \"::notice::jactionlint $v on $p has no Velnor executable digest; the Mise release digest and GitHub attestation checks still applied\"; exit 0; fi"
        ),
        "d=\"$RUNNER_TEMP/velnor/jactionlint-verify\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        BIND_SELECTED_MISE_FUNCTION.to_owned(),
        format!(
            "if velnor_bind_selected_mise \"$d/mise-data-dir\"; then :; else bind_status=$?; case \"$bind_status\" in 1) {fail}jactionlint_mise_data_dir_missing: no Setup Mise data directory is selected\" >&2;; 2) {fail}jactionlint_mise_data_dir_invalid: Setup Mise data directory is missing\" >&2;; 3) {fail}jactionlint_mise_data_dir_unresolvable: Setup Mise data directory cannot be resolved\" >&2;; *) {fail}jactionlint_mise_binary_invalid: Setup Mise binary is missing, not executable, or a symlink\" >&2;; esac; exit 1; fi"
        ),
        format!(
            "MISE_DATA_DIR=\"$resolved_mise_data_dir\" \"$mise_bin\" --no-config --no-env --no-hooks which --tool \"jactionlint@$v\" jactionlint > \"$d/path\" || {{ {fail}jactionlint_not_installed: jactionlint@$v\" >&2; exit 1; }}"
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
