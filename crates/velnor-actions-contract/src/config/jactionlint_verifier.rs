// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::selected_mise::BIND_SELECTED_MISE_FUNCTION;

pub const JACTIONLINT_VERSION: &str = "2.5.0";

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
        version: "2.5.0",
        platform: "linux-arm64",
        asset: "jactionlint_2.5.0_linux_arm64.tar.gz",
        archive_sha256: "ad7e33b6264cea16b7e83ad2bb004824d5b1c15aedb35ba0dc84917f791c5525",
        executable_sha256: "7875208f28ba3fac6a9dae33831e234abe2be5bd6248a29fa0b245b670d9bc55",
    },
    JactionlintDigest {
        version: "2.5.0",
        platform: "linux-x64",
        asset: "jactionlint_2.5.0_linux_amd64.tar.gz",
        archive_sha256: "5ac307b0499bb0eab45ae99c23e8e0790874fbc667c75224410954e0e3c6e0f5",
        executable_sha256: "f2b178cac570e47923da59bd4a12ba376917d4430f171fb872657906be437f6c",
    },
    JactionlintDigest {
        version: "2.5.0",
        platform: "macos-arm64",
        asset: "jactionlint_2.5.0_darwin_arm64.tar.gz",
        archive_sha256: "15b47d53e3b1aafbd674cca0e8fff0d5952f3cf2a7d7501a8f67576ef76397b4",
        executable_sha256: "73109c50a819c00d816fa9a0ed3ec6b49233060b617a4a2b27438a437b063020",
    },
    JactionlintDigest {
        version: "2.5.0",
        platform: "macos-x64",
        asset: "jactionlint_2.5.0_darwin_amd64.tar.gz",
        archive_sha256: "f8f10def21260aa9515974f0aa145f5a6741efd60efe1f880c8c6f53a4ba676c",
        executable_sha256: "dafad00a45b791c4e8cbaa4cc7d8f55e923a7605a5b677194b8c6a29ee0997d5",
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
