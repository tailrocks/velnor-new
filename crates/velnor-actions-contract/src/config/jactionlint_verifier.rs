// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::selected_mise::BIND_SELECTED_MISE_FUNCTION;

pub const JACTIONLINT_VERSION: &str = "2.4.0";

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
        version: "2.4.0",
        platform: "linux-arm64",
        asset: "jactionlint_2.4.0_linux_arm64.tar.gz",
        archive_sha256: "42413391c248f99ff4550d6c866ee717d5ea4ca2b966d9d54ad1503af955e22f",
        executable_sha256: "5be2560b719b1764a7ff9a5983cd47dd0bd0f538cb35baed8d598c536e0cee35",
    },
    JactionlintDigest {
        version: "2.4.0",
        platform: "linux-x64",
        asset: "jactionlint_2.4.0_linux_amd64.tar.gz",
        archive_sha256: "26ddba81db28fffc51163175c070ee1a9d1150260de4b5232f01d9b749f32d8e",
        executable_sha256: "70e41eb37cdd89a305f9a433e230976732fe2ef5f674bbea8d68322ae583bdd0",
    },
    JactionlintDigest {
        version: "2.4.0",
        platform: "macos-arm64",
        asset: "jactionlint_2.4.0_darwin_arm64.tar.gz",
        archive_sha256: "349a13dbffdd25376898a256d195806afda08ba97ccc6378cbf2dd11a77ef8e7",
        executable_sha256: "4dff3e799bd47c5b19363c1c7d6f86d09cc2ce41112aede55fb1bfc982eb8643",
    },
    JactionlintDigest {
        version: "2.4.0",
        platform: "macos-x64",
        asset: "jactionlint_2.4.0_darwin_amd64.tar.gz",
        archive_sha256: "4c21fce834f8267b65a65b32a527aaa2ec1c321c8f066d44b42d7f038835aa90",
        executable_sha256: "39bbd9e4bf6302fa92bc12cebf933b26fad58b6e76d8468c464ae3880f2ef453",
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
