//! Hosted measurement of the selected official Rust distribution components.

use super::{RunnerSpec, RustToolchainQualificationPins};
use crate::RenderError;
use crate::schema2_features::{self, CHECKOUT_USES};
use crate::yaml::Yaml;
use velnor_actions_contract::ReleaseTarget;

const MODE: &str = "inputs.mode == 'rust-toolchain'";
const UPLOAD_USES: &str = "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a";
pub(super) const MANIFEST_SHA256: &str =
    "ce6dddc886364f8d786514771212cebe9b731ba82d6b859951c6b0ccc516b6a2";
const RUST_VERSION: &str = "1.99.0";

/// Emit read-only qualification jobs on Linux x64 and macOS ARM64.
///
/// # Errors
/// Invalid version, manifest URL, or manifest digest fails closed.
pub(super) fn jobs(
    pins: &RustToolchainQualificationPins,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    validate_pins(pins)?;
    Ok(vec![
        job(
            "rust-toolchain-linux-x64",
            "Rust toolchain / hosted / Linux x64",
            "ubuntu-26.04",
            ReleaseTarget::LinuxX86_64,
            "linux_x64",
            "rust-toolchain-linux-x64",
            pins,
        )?,
        job(
            "rust-toolchain-macos-arm64",
            "Rust toolchain / hosted / macOS ARM64",
            "macos-26",
            ReleaseTarget::MacosArm64,
            "macos_arm64",
            "rust-toolchain-macos-arm64",
            pins,
        )?,
    ])
}

fn validate_pins(pins: &RustToolchainQualificationPins) -> Result<(), RenderError> {
    let expected_url = format!(
        "https://static.rust-lang.org/dist/channel-rust-{}.toml",
        pins.rust_version
    );
    let digest_valid = pins.manifest_sha256.len() == 64
        && pins
            .manifest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && pins.manifest_sha256.bytes().any(|byte| byte != b'0');
    if pins.rust_version != RUST_VERSION
        || pins.manifest_url != expected_url
        || pins.manifest_sha256 != MANIFEST_SHA256
        || !digest_valid
    {
        return Err(RenderError::BadCommand(
            "rust_toolchain_qualification_pin_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn job(
    id: &str,
    name: &str,
    runner: &str,
    target: ReleaseTarget,
    platform: &str,
    artifact_name: &str,
    pins: &RustToolchainQualificationPins,
) -> Result<(String, Yaml), RenderError> {
    let hosted = RunnerSpec::hosted_release_target(runner, target)?;
    let mut fields = schema2_features::lane_base(name, &hosted, 30);
    fields.insert(1, ("if".to_owned(), Yaml::str(MODE)));
    fields.push(("permissions".to_owned(), mapping(&[("contents", "read")])));
    Ok(schema2_features::finish(
        id,
        fields,
        vec![
            checkout_step(),
            probe_step(platform, pins),
            upload_step(artifact_name),
        ],
    ))
}

fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out selected ref")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            mapping(&[
                ("ref", "${{ github.sha }}"),
                ("persist-credentials", "false"),
            ]),
        ),
    ])
}

fn probe_step(platform: &str, pins: &RustToolchainQualificationPins) -> Yaml {
    let run = concat!(
        "set -euo pipefail\n",
        "python3 --version\n",
        "python3 scripts/qualification/qualify_rust_toolchain.py \\\n",
        "  --version \"$RUST_VERSION\" \\\n",
        "  --manifest-url \"$RUST_MANIFEST_URL\" \\\n",
        "  --manifest-sha256 \"$RUST_MANIFEST_SHA256\" \\\n",
        "  --platform \"$QUALIFICATION_PLATFORM\" \\\n",
        "  --output \"$QUALIFICATION_OUTPUT\""
    );
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Install official Rust components and measure the qualified tree"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            mapping(&[
                ("RUST_VERSION", &pins.rust_version),
                ("RUST_MANIFEST_URL", &pins.manifest_url),
                ("RUST_MANIFEST_SHA256", &pins.manifest_sha256),
                ("QUALIFICATION_PLATFORM", platform),
                (
                    "QUALIFICATION_OUTPUT",
                    "${{ runner.temp }}/rust-toolchain-qualification.json",
                ),
            ]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

fn upload_step(name: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Upload measured Rust qualification"),
        ),
        ("uses".to_owned(), Yaml::str(UPLOAD_USES)),
        (
            "with".to_owned(),
            mapping(&[
                ("name", name),
                (
                    "path",
                    "${{ runner.temp }}/rust-toolchain-qualification.json",
                ),
                ("if-no-files-found", "error"),
                ("retention-days", "14"),
            ]),
        ),
    ])
}

fn mapping(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

#[cfg(test)]
#[path = "schema2_rust_toolchain_qualification_tests.rs"]
mod tests;
