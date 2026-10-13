//! Source-bound macOS host and verification-helper release workflow.

use crate::commands::join_argv_for_run;
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;
use crate::{RenderError, schema2::ProductReleasePins};
use velnor_actions_contract::ReleaseTarget;

use super::super::Schema2WorkflowRequest;
use super::super::features::run_step;
use super::{Publish, attest_job, build_job, document, publish_job};

const MACOS_RUNS_ON: &str = "macos-15";
const HOST_BIN: &str = "velnor-host";
const HELPER_BIN: &str = "velnor-runner-attestation-helper";
const RELEASE_MANIFEST: &str = "BINARY_RELEASE_MANIFEST.json";
const CHECKSUMS: &str = "SHA256SUMS";
const RELEASE_ASSETS: &[&str] = &[HOST_BIN, HELPER_BIN, RELEASE_MANIFEST, CHECKSUMS];

const HELPER_BUILD_SUFFIX: &str = r#"cp crates/velnor-runner/target/release/velnor-runner-attestation-helper velnor-runner-attestation-helper
test -s velnor-runner-attestation-helper
description="$(file -b velnor-runner-attestation-helper)"
case "$description" in
  *Mach-O*arm64*) ;;
  *) echo "not an arm64 Mach-O: $description" >&2; exit 1 ;;
esac
helper_sha256="$(shasum -a 256 velnor-runner-attestation-helper | awk 'NR == 1 { print $1; next } END { if (NR != 1) exit 1 }')"
helper_size="$(stat -f '%z' velnor-runner-attestation-helper)"
[[ "$helper_sha256" =~ ^[0-9a-f]{64}$ ]]
[[ "$helper_size" =~ ^[1-9][0-9]*$ ]]
printf 'sha256=%s\nsize=%s\n' "$helper_sha256" "$helper_size" >> "$GITHUB_OUTPUT"
"#;

const HOST_BUILD_PREFIX: &str = r#"[[ "$VELNOR_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]]
[[ "$VELNOR_ATTESTATION_HELPER_SHA256" =~ ^[0-9a-f]{64}$ ]]"#;

const HOST_BUILD_SUFFIX: &str = r"cp crates/velnor-runner/target/release/velnor-host velnor-host
test -s velnor-host";

const MANIFEST_AND_CHECKSUMS: &str = r#"set -eu
source_sha="$VELNOR_SOURCE_SHA"
authority_sha="$VELNOR_WORKFLOW_AUTHORITY_SHA"
[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]]
[[ "$authority_sha" == "$source_sha" ]]
[[ "$VELNOR_EXPECTED_HELPER_SHA256" =~ ^[0-9a-f]{64}$ ]]
[[ "$VELNOR_EXPECTED_HELPER_SIZE" =~ ^[1-9][0-9]*$ ]]
verify_arm64_macho() {
  local description
  description="$(file -b "$1")"
  case "$description" in
    *Mach-O*arm64*) ;;
    *) echo "not an arm64 Mach-O: $description" >&2; exit 1 ;;
  esac
}
test -s velnor-host
test -s velnor-runner-attestation-helper
verify_arm64_macho velnor-host
verify_arm64_macho velnor-runner-attestation-helper
host_sha256="$(shasum -a 256 velnor-host | awk 'NR == 1 { print $1; next } END { if (NR != 1) exit 1 }')"
helper_sha256="$(shasum -a 256 velnor-runner-attestation-helper | awk 'NR == 1 { print $1; next } END { if (NR != 1) exit 1 }')"
host_size="$(stat -f '%z' velnor-host)"
helper_size="$(stat -f '%z' velnor-runner-attestation-helper)"
[[ "$host_sha256" =~ ^[0-9a-f]{64}$ ]]
[[ "$helper_sha256" == "$VELNOR_EXPECTED_HELPER_SHA256" ]]
[[ "$helper_size" == "$VELNOR_EXPECTED_HELPER_SIZE" ]]
[[ "$host_size" =~ ^[1-9][0-9]*$ ]]
printf '{"schema_version":1,"repository":"tailrocks/velnor-new","source_ref":"refs/heads/main","source_commit":"%s","signer_workflow":".github/workflows/product-release-binary.yml","signer_ref":"refs/heads/main","workflow_authority_sha":"%s","trusted_signing_identity":{"oidc_issuer":"https://token.actions.githubusercontent.com","certificate_identity":"https://github.com/tailrocks/velnor-new/.github/workflows/product-release-binary.yml@refs/heads/main"},"platform":"macos/arm64","host":{"name":"velnor-host","target":"aarch64-apple-darwin","sha256":"%s","size":%s},"attestation_helper":{"name":"velnor-runner-attestation-helper","target":"aarch64-apple-darwin","sha256":"%s","size":%s}}\n' "$source_sha" "$authority_sha" "$host_sha256" "$host_size" "$helper_sha256" "$helper_size" > BINARY_RELEASE_MANIFEST.json
shasum -a 256 velnor-host velnor-runner-attestation-helper BINARY_RELEASE_MANIFEST.json > SHA256SUMS"#;

/// Render the binary family after validating all pinned command vectors.
///
/// # Errors
///
/// Missing pins, unsafe commands, or an illegal macOS runner fail.
pub(super) fn macos_binary_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("product_release_pins_missing".to_owned()))?;
    let install = join_argv_for_run(&pins.install_runner_build_tools_argv)?;
    let macos = runs_on_yaml(MACOS_RUNS_ON)?;
    Ok(document(
        "macOS binary release",
        vec![
            build_job(
                "build-binary",
                "Build Velnor binaries",
                macos.clone(),
                120,
                vec![
                    super::super::generator_release::mise_setup_step(
                        pins,
                        ReleaseTarget::MacosArm64,
                    )?,
                    run_step("Install pinned Rust", &install),
                    helper_build_step(pins)?,
                    host_build_step(pins)?,
                    manifest_step(),
                ],
                "Upload binary assets",
                RELEASE_ASSETS,
            ),
            attest_job(
                "attest-binary",
                "Attest Velnor binaries",
                macos.clone(),
                "build-binary",
                "binary-assets",
                RELEASE_ASSETS,
            ),
            publish_job(
                macos,
                &Publish {
                    id: "publish-binary",
                    name: "Publish Velnor binaries",
                    needs: "attest-binary",
                    artifact: "binary-assets",
                    prefix: "binary",
                    notes: "Velnor binaries built from the accepted source commit.",
                    files: RELEASE_ASSETS,
                },
            ),
        ],
    ))
}

fn helper_build_step(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    let argv = join_argv_for_run(&pins.runner_attestation_helper_build_argv)?;
    let run = format!("set -eu\n{argv}\n{HELPER_BUILD_SUFFIX}");
    Ok(output_step("Build attestation helper", "helper", &run))
}

fn host_build_step(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    let argv = join_argv_for_run(&pins.runner_build_argv)?;
    let run = format!("set -eu\n{HOST_BUILD_PREFIX}\n{argv}\n{HOST_BUILD_SUFFIX}");
    Ok(env_step(
        "Build velnor-host",
        &[
            ("VELNOR_SOURCE_SHA", "${{ inputs.source_sha }}"),
            (
                "VELNOR_ATTESTATION_HELPER_SHA256",
                "${{ steps.helper.outputs.sha256 }}",
            ),
        ],
        &run,
    ))
}

fn manifest_step() -> Yaml {
    env_step(
        "Write binary manifest and checksums",
        &[
            (
                "VELNOR_EXPECTED_HELPER_SHA256",
                "${{ steps.helper.outputs.sha256 }}",
            ),
            (
                "VELNOR_EXPECTED_HELPER_SIZE",
                "${{ steps.helper.outputs.size }}",
            ),
        ],
        MANIFEST_AND_CHECKSUMS,
    )
}

fn output_step(name: &str, id: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

fn env_step(name: &str, env: &[(&str, &str)], run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        (
            "env".to_owned(),
            Yaml::Map(
                env.iter()
                    .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
                    .collect(),
            ),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

#[cfg(test)]
#[path = "schema2_release_binary_tests.rs"]
mod tests;
