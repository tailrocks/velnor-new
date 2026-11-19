// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::selected_mise::BIND_SELECTED_MISE_FUNCTION;

pub const JACTIONLINT_VERSION: &str = "2.6.0";

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
        version: "2.6.0",
        platform: "linux-arm64",
        asset: "jactionlint_2.6.0_linux_arm64.tar.gz",
        archive_sha256: "c726185a85f26d09aa94ea556509a19827979074b35d65f357f3a4947573e934",
        executable_sha256: "57a5df96bc89edeb0aba733d2e6f853fabe51a9974e51eba1aeb5a057dd33b5e",
    },
    JactionlintDigest {
        version: "2.6.0",
        platform: "linux-x64",
        asset: "jactionlint_2.6.0_linux_amd64.tar.gz",
        archive_sha256: "d5400967b8f0d3a5ae9a4cee21f56400a50eb3f05d0cbb4ef17184f17f0760c8",
        executable_sha256: "0793a4bcfd85a1cdf89296038fa86222c7a630bac2e7f0c988cbf2eb55f368c9",
    },
    JactionlintDigest {
        version: "2.6.0",
        platform: "macos-arm64",
        asset: "jactionlint_2.6.0_darwin_arm64.tar.gz",
        archive_sha256: "c3f373c7eac668aacb390b9c802cf4b5e2ad1a11b0b0c0aea6b04ca0cfa0c018",
        executable_sha256: "5dcd300c53abadb27c824d48d2d32a911e248344ce7f03f41d7d728fc0389db9",
    },
    JactionlintDigest {
        version: "2.6.0",
        platform: "macos-x64",
        asset: "jactionlint_2.6.0_darwin_amd64.tar.gz",
        archive_sha256: "aa89c49ad5c9530f2308f18333166050f97f14d420f478a8225d043bef1e2cab",
        executable_sha256: "9977d023ce27b6a9943f2e5e2e9c575581a9f4240b54c014bd3fe487d3ebbdcc",
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
