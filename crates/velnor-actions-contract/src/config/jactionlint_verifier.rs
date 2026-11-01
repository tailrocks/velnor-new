// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::selected_mise::BIND_SELECTED_MISE_FUNCTION;

pub const JACTIONLINT_VERSION: &str = "2.3.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JactionlintDigest {
    pub version: &'static str,

    pub platform: &'static str,

    pub asset: &'static str,

    pub archive_sha256: &'static str,

    pub executable_sha256: &'static str,
}

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

#[must_use]
pub fn jactionlint_archive_sha256(version: &str, platform: &str) -> Option<&'static str> {
    row(version, platform).map(|row| row.archive_sha256)
}

#[must_use]
pub fn jactionlint_executable_sha256(version: &str, platform: &str) -> Option<&'static str> {
    row(version, platform).map(|row| row.executable_sha256)
}

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
