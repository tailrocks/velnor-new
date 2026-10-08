//! Release candidate builds, target assets, and qualification.

use velnor_actions_contract_release::ReleaseTarget;

use velnor_actions_workflow_tree::yaml::Yaml;

use super::archive;
use super::workflow_steps::{self, checkout_step, with_permissions};
use velnor_actions_workflow_tree::job_entries::{base, finish};

pub(super) const VERSION: &str = "0.1.1";
pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";
const CI_WORKFLOW: &str = "ci.yml";

/// One target binary, checksum sidecar, and source-bound build record.
#[derive(Clone, Copy)]
pub(super) struct ProductAsset {
    pub target: ReleaseTarget,
    pub binary: &'static str,
    pub sidecar: &'static str,
    pub provenance: &'static str,
    pub archive: &'static str,
    pub workflow_artifact: &'static str,
    pub directory: &'static str,
    pub build_job: &'static str,
    pub qualify_job: &'static str,
    pub upload_name: &'static str,
    pub sum_command: &'static str,
    pub checksum_command: &'static str,
}

/// Linux `x86_64` binary.
pub(super) const LINUX: ProductAsset = ProductAsset {
    target: ReleaseTarget::LinuxX86_64,
    binary: "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
    sidecar: "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.sha256",
    provenance: "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.provenance.json",
    archive: "generator-linux-assets.tar",
    workflow_artifact: "generator-linux-assets",
    directory: "linux-assets",
    build_job: "build-linux",
    qualify_job: "qualify-linux",
    upload_name: "Upload Linux assets",
    sum_command: "sha256sum",
    checksum_command: "sha256sum --check",
};

/// macOS arm64 binary.
pub(super) const MACOS_ARM64: ProductAsset = ProductAsset {
    target: ReleaseTarget::MacosArm64,
    binary: "velnor-actions-0.1.1-aarch64-apple-darwin",
    sidecar: "velnor-actions-0.1.1-aarch64-apple-darwin.sha256",
    provenance: "velnor-actions-0.1.1-aarch64-apple-darwin.provenance.json",
    archive: "generator-macos-assets.tar",
    workflow_artifact: "generator-macos-assets",
    directory: "macos-assets",
    build_job: "build-macos",
    qualify_job: "qualify-macos",
    upload_name: "Upload macOS assets",
    sum_command: "shasum -a 256",
    checksum_command: "shasum -a 256 --check",
};

/// macOS `x86_64` binary.
pub(super) const MACOS_X86_64: ProductAsset = ProductAsset {
    target: ReleaseTarget::MacosX86_64,
    binary: "velnor-actions-0.1.1-x86_64-apple-darwin",
    sidecar: "velnor-actions-0.1.1-x86_64-apple-darwin.sha256",
    provenance: "velnor-actions-0.1.1-x86_64-apple-darwin.provenance.json",
    archive: "generator-macos-intel-assets.tar",
    workflow_artifact: "generator-macos-intel-assets",
    directory: "macos-intel-assets",
    build_job: "build-macos-intel",
    qualify_job: "qualify-macos-intel",
    upload_name: "Upload macOS x86_64 assets",
    sum_command: "shasum -a 256",
    checksum_command: "shasum -a 256 --check",
};

pub(super) const ASSETS: [ProductAsset; 3] = [LINUX, MACOS_ARM64, MACOS_X86_64];

/// Build, inspect, and checksum one candidate before uploading it.
pub(super) fn build_steps(
    product: ProductAsset,
    build_host: ReleaseTarget,
    verify_name: &str,
    verify: &str,
    pins: &crate::generator_release_pins::GeneratorReleasePins,
) -> Result<Vec<Yaml>, velnor_actions_workflow_steps::RenderError> {
    let (build_argv, binary_path, prepare_target) = match (product.target, build_host) {
        (target, host) if target == host => (
            pins.build_argv.as_slice(),
            "target/release/velnor-actions".to_owned(),
            None,
        ),
        (ReleaseTarget::MacosX86_64, ReleaseTarget::MacosArm64) => (
            pins.macos_x86_64_cross_build_argv.as_slice(),
            format!(
                "target/{}/release/velnor-actions",
                ReleaseTarget::MacosX86_64.triple()
            ),
            Some(pins.install_macos_x86_64_target_argv.as_slice()),
        ),
        (target, host) => {
            return Err(velnor_actions_workflow_steps::RenderError::InvalidWorkflow(
                format!(
                    "generator_release_unsupported_build_host:{}:{}",
                    host.triple(),
                    target.triple()
                ),
            ));
        }
    };
    let build = build_script(product.binary, &binary_path, build_argv)?;
    let build_name = if prepare_target.is_some() {
        "Cross-build velnor-actions with MBX"
    } else {
        "Build velnor-actions with MBX"
    };
    let mut steps = vec![workflow_steps::mise_step(pins.setup_for(build_host))?];
    steps.push(workflow_steps::command_step(
        "Install pinned Rust and MBX",
        &pins.install_build_tools_argv,
    )?);
    if let Some(argv) = prepare_target {
        steps.push(workflow_steps::command_step("Prepare Rust target", argv)?);
    }
    steps.extend([
        workflow_steps::bash_step(build_name, &build),
        workflow_steps::bash_step(verify_name, verify),
        workflow_steps::bash_step(
            "Checksum built bytes",
            &sum_script(product.sum_command, product.binary, product.sidecar),
        ),
        workflow_steps::bash_step(
            "Record candidate provenance",
            &candidate_provenance_script(product, pins),
        ),
        workflow_steps::bash_step(
            "Package candidate preserving executable mode",
            &archive_script(product),
        ),
    ]);
    Ok(steps)
}

/// The build emits the filename recorded in the release manifest.
fn build_script(
    asset: &str,
    binary_path: &str,
    build_argv: &[String],
) -> Result<String, velnor_actions_workflow_steps::RenderError> {
    Ok(format!(
        "set -eu\nenv -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN -u MISE_GITHUB_TOKEN -u GH_TOKEN -u GH_HOST -u GH_CONFIG_DIR {}\ncp {binary_path} {asset}\ntest -s {asset}",
        velnor_actions_workflow_steps::commands::join_argv_for_run(build_argv)?
    ))
}

/// Require the same ELF architecture that the target triple names.
pub(super) fn linux_verify(asset: &str) -> String {
    format!(
        "set -eu\ndesc=\"$(file -b {asset})\"\ncase \"$desc\" in\n  *ELF*x86-64*) ;;\n  *) echo \"not an x86-64 ELF: $desc\" >&2; exit 1 ;;\nesac"
    )
}

/// Require the requested Mach-O architecture on native macOS runners.
pub(super) fn macos_verify(asset: &str, arch: &str) -> String {
    format!(
        "set -eu\ndesc=\"$(file -b {asset})\"\ncase \"$desc\" in\n  *Mach-O*{arch}*) ;;\n  *) echo \"not a {arch} Mach-O: $desc\" >&2; exit 1 ;;\nesac"
    )
}

fn sum_script(command: &str, asset: &str, sidecar: &str) -> String {
    format!("set -eu\n{command} {asset} > {sidecar}")
}

fn candidate_provenance_script(
    product: ProductAsset,
    pins: &crate::generator_release_pins::GeneratorReleasePins,
) -> String {
    let digest = archive::sidecar_digest_command(product.sidecar, product.binary);
    format!(
        "set -eu\ndigest=\"$({digest})\"\njq -n --arg repository \"$GITHUB_REPOSITORY\" --arg commit \"$GITHUB_SHA\" --arg target \"{}\" --arg asset \"{}\" --arg sha256 \"$digest\" --arg rust '{}' --arg mr_boxington '{}' '{{\"schema\":1,\"version\":\"{VERSION}\",\"repository\":$repository,\"commit\":$commit,\"target\":$target,\"asset\":$asset,\"sha256\":$sha256,\"toolchain\":{{\"rust\":$rust,\"mr-boxington\":$mr_boxington}}}}' > {}\nchmod 755 {}\nchmod 644 {} {}\ntest -s {}\ntest -s {}",
        product.target.triple(),
        product.binary,
        pins.rust_version,
        pins.mr_boxington_version,
        product.provenance,
        product.binary,
        product.sidecar,
        product.provenance,
        product.provenance,
        product.binary
    )
}

fn archive_script(product: ProductAsset) -> String {
    format!(
        "set -eu\ntar -cf {} {} {} {}\ntest -s {}",
        product.archive, product.binary, product.sidecar, product.provenance, product.archive
    )
}

/// Download the exact immutable artifact produced by one build job.
pub(super) fn download_build_steps(asset: ProductAsset, name: &str) -> Vec<Yaml> {
    download_build_steps_for_id(asset, name, "${{ inputs.artifact_id }}")
}

/// Download one immutable build artifact selected by the producer's ID.
pub(super) fn download_build_steps_for_id(
    asset: ProductAsset,
    name: &str,
    artifact_id: &str,
) -> Vec<Yaml> {
    download_steps_with(
        asset,
        workflow_steps::download_step_by_id(name, artifact_id, asset.directory),
    )
}

fn download_steps_with(asset: ProductAsset, download: Yaml) -> Vec<Yaml> {
    vec![
        download,
        workflow_steps::bash_step(
            "Prevalidate and extract uploaded candidate archive",
            &archive::extraction_script(
                asset.directory,
                asset.archive,
                asset.binary,
                asset.sidecar,
                asset.provenance,
            ),
        ),
        workflow_steps::bash_step(
            "Verify extracted candidate file types and executable mode",
            &format!(
                "set -eu\ntest -f {}/{}\ntest ! -L {}/{}\ntest -x {}/{}\ntest -f {}/{}\ntest ! -L {}/{}\ntest ! -x {}/{}\ntest -f {}/{}\ntest ! -L {}/{}\ntest ! -x {}/{}",
                asset.directory,
                asset.binary,
                asset.directory,
                asset.binary,
                asset.directory,
                asset.binary,
                asset.directory,
                asset.sidecar,
                asset.directory,
                asset.sidecar,
                asset.directory,
                asset.sidecar,
                asset.directory,
                asset.provenance,
                asset.directory,
                asset.provenance,
                asset.directory,
                asset.provenance
            ),
        ),
    ]
}

/// Verify source, target, digest, and exact toolchain recorded in the candidate archive.
pub(super) fn verify_provenance_script(
    asset: ProductAsset,
    pins: &crate::generator_release_pins::GeneratorReleasePins,
) -> String {
    verify_provenance_in_directory(
        asset,
        asset.directory,
        &pins.rust_version,
        &pins.mr_boxington_version,
    )
}

fn verify_provenance_in_directory(
    asset: ProductAsset,
    directory: &str,
    rust_version: &str,
    mr_boxington_version: &str,
) -> String {
    let digest =
        archive::sidecar_digest_command(&format!("{directory}/{}", asset.sidecar), asset.binary);
    format!(
        "set -eu\ndigest=\"$({digest})\"\njq -e --arg version \"{VERSION}\" --arg repository \"$GITHUB_REPOSITORY\" --arg commit \"$GITHUB_SHA\" --arg target \"{}\" --arg asset \"{}\" --arg sha256 \"$digest\" --arg rust '{}' --arg mr_boxington '{}' '.schema == 1 and .version == $version and .repository == $repository and .commit == $commit and .target == $target and .asset == $asset and .sha256 == $sha256 and .toolchain.rust == $rust and .toolchain[\"mr-boxington\"] == $mr_boxington' {}/{} > /dev/null",
        asset.target.triple(),
        asset.binary,
        rust_version,
        mr_boxington_version,
        directory,
        asset.provenance
    )
}

/// Check candidate identity and deterministic generation on the uploaded bytes.
pub(super) fn qualification_script(binary: &str, directory: &str) -> String {
    format!(
        "set -eu\nunset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN GH_TOKEN MISE_GITHUB_TOKEN GH_HOST GH_CONFIG_DIR\ntest \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\ntest \"$(./{directory}/{binary} --version)\" = \"velnor-actions {VERSION}\"\nscripts/capture-opentofu-goldens.sh check-release \"$GITHUB_WORKSPACE/{directory}/{binary}\" \"$GITHUB_WORKSPACE/manifest-assets/release-manifest.json\" \"$VELNOR_RELEASE_MANIFEST_SHA256\""
    )
}

/// Check the dispatched commit and required CI before building candidates.
pub(super) fn source_gate_job(
    hosted: Yaml,
    pins: &crate::generator_release_pins::GeneratorReleasePins,
) -> Result<(String, Yaml), velnor_actions_workflow_steps::RenderError> {
    let steps = vec![
        checkout_step(),
        workflow_steps::mise_step(pins.setup_for(ReleaseTarget::LinuxX86_64))?,
        workflow_steps::command_step(
            "Install pinned release gate tools",
            &pins.install_gate_tools_argv,
        )?,
        workflow_steps::command_step("Run actionlint", &pins.actionlint_argv)?,
        workflow_steps::command_step("Run zizmor", &pins.zizmor_argv)?,
        ci_check_step(pins)?,
        workflow_steps::bash_step(
            "Check release freshness gate",
            "bash scripts/check-freshness.sh",
        ),
        workflow_steps::bash_step_with_token(
            "Recheck default-branch source",
            &format!(
                "set -eu\ntest \"$GITHUB_REF\" = \"refs/heads/main\"\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\ntest \"$(gh api repos/{REPOSITORY}/commits/main --jq .sha)\" = \"$GITHUB_SHA\""
            ),
            &pins.gh_argv,
        )?,
    ];
    let mut fields = with_permissions(
        base("Verify generator release source", hosted, 20),
        workflow_steps::perm(&[("actions", "read"), ("contents", "read")]),
    );
    fields.retain(|(key, _)| key != "name");
    Ok(finish("verify-release-source", fields, steps))
}

fn ci_check_step(
    pins: &crate::generator_release_pins::GeneratorReleasePins,
) -> Result<Yaml, velnor_actions_workflow_steps::RenderError> {
    workflow_steps::bash_step_with_token(
        "Require successful CI at exact main SHA",
        &ci_check_script(),
        &pins.gh_argv,
    )
}

pub(super) fn ci_check_script() -> String {
    format!(
        r#"set -eu
test "$GITHUB_REPOSITORY" = '{REPOSITORY}'
test "$GITHUB_REF" = 'refs/heads/main'
test "$GITHUB_EVENT_NAME" = 'workflow_dispatch'
test "$GITHUB_WORKFLOW_SHA" = "$GITHUB_SHA"
test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
runs="$(gh api --paginate --slurp "repos/{REPOSITORY}/actions/workflows/{CI_WORKFLOW}/runs?head_sha=$GITHUB_SHA&branch=main&event=push&per_page=100")"
run="$(printf '%s\n' "$runs" | jq -ce --arg sha "$GITHUB_SHA" --arg repo '{REPOSITORY}' '[.[] | (.workflow_runs // [])[] | select(.path == ".github/workflows/{CI_WORKFLOW}" and .head_sha == $sha and .head_branch == "main" and .head_repository.full_name == $repo and .event == "push")] | sort_by([.run_number, .id]) | last // error("no exact-source main CI run")')"
test "$(printf '%s\n' "$run" | jq -r .status)" = completed
test "$(printf '%s\n' "$run" | jq -r .conclusion)" = success
run_id="$(printf '%s\n' "$run" | jq -r .id)"
attempt="$(printf '%s\n' "$run" | jq -r .run_attempt)"
jobs="$(gh api --paginate --slurp "repos/{REPOSITORY}/actions/runs/$run_id/attempts/$attempt/jobs?per_page=100")"
printf '%s\n' "$jobs" | jq -e --arg sha "$GITHUB_SHA" '[.[] | (.jobs // [])[] | select(.name == "Required" and .head_sha == $sha and .head_branch == "main" and .status == "completed" and .conclusion == "success")] | length == 1' >/dev/null
test "$(gh api repos/{REPOSITORY}/commits/main --jq .sha)" = "$GITHUB_SHA""#
    )
}

#[cfg(test)]
mod tests;
