//! Release candidate builds, target assets, and qualification.

use crate::yaml::Yaml;

use super::super::features::{base, finish};
use super::archive;
use super::workflow_steps::{self, checkout_step, with_permissions};

pub(super) const VERSION: &str = "0.1.1";
pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";
const CI_WORKFLOW: &str = "ci.yml";
pub(super) const GH_VERSION: &str = "2.102.0";

pub(super) fn pinned_gh_prefix() -> String {
    format!("mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh ")
}

pub(super) fn pinned_gh(arguments: &str) -> String {
    format!("{}{arguments}", pinned_gh_prefix())
}

pub(super) fn install_pinned_gh() -> String {
    format!("mise --no-config --no-env --no-hooks install gh@{GH_VERSION}")
}

/// One target binary, checksum sidecar, and source-bound build record.
#[derive(Clone, Copy)]
pub(super) struct ProductAsset {
    pub target: &'static str,
    pub binary: &'static str,
    pub sidecar: &'static str,
    pub provenance: &'static str,
    pub archive: &'static str,
    pub workflow_artifact: &'static str,
    pub directory: &'static str,
    pub build_job: &'static str,
    pub qualify_job: &'static str,
    pub attest_job: &'static str,
    pub upload_name: &'static str,
    pub sum_command: &'static str,
    pub checksum_command: &'static str,
}

/// Linux `x86_64` binary.
pub(super) const LINUX: ProductAsset = ProductAsset {
    target: "x86_64-unknown-linux-gnu",
    binary: "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
    sidecar: "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.sha256",
    provenance: "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.provenance.json",
    archive: "generator-linux-assets.tar",
    workflow_artifact: "generator-linux-assets",
    directory: "linux-assets",
    build_job: "build-linux",
    qualify_job: "qualify-linux",
    attest_job: "attest-linux",
    upload_name: "Upload Linux assets",
    sum_command: "sha256sum",
    checksum_command: "sha256sum --check",
};

/// macOS arm64 binary.
pub(super) const MACOS_ARM64: ProductAsset = ProductAsset {
    target: "aarch64-apple-darwin",
    binary: "velnor-actions-0.1.1-aarch64-apple-darwin",
    sidecar: "velnor-actions-0.1.1-aarch64-apple-darwin.sha256",
    provenance: "velnor-actions-0.1.1-aarch64-apple-darwin.provenance.json",
    archive: "generator-macos-assets.tar",
    workflow_artifact: "generator-macos-assets",
    directory: "macos-assets",
    build_job: "build-macos",
    qualify_job: "qualify-macos",
    attest_job: "attest-macos",
    upload_name: "Upload macOS assets",
    sum_command: "shasum -a 256",
    checksum_command: "shasum -a 256 --check",
};

pub(super) const ASSETS: [ProductAsset; 2] = [LINUX, MACOS_ARM64];

/// Build, inspect, and checksum one native candidate before uploading it.
pub(super) fn build_steps(product: ProductAsset, verify_name: &str, verify: &str) -> Vec<Yaml> {
    vec![
        workflow_steps::mise_step(),
        workflow_steps::bash_step(
            "Install pinned Rust and MBX",
            "mise --no-config --no-env --no-hooks install rust@1.98.1 mr-boxington@1.21.1",
        ),
        workflow_steps::bash_step(
            "Build velnor-actions with MBX",
            &build_script(product.binary),
        ),
        workflow_steps::bash_step(verify_name, verify),
        workflow_steps::bash_step(
            "Checksum built bytes",
            &sum_script(product.sum_command, product.binary, product.sidecar),
        ),
        workflow_steps::bash_step(
            "Record candidate provenance",
            &candidate_provenance_script(product),
        ),
        workflow_steps::bash_step(
            "Package candidate preserving executable mode",
            &archive_script(product),
        ),
    ]
}

/// The build emits the filename recorded in the release manifest.
fn build_script(asset: &str) -> String {
    format!(
        "set -eu\nenv -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN -u MISE_GITHUB_TOKEN -u GH_TOKEN -u GH_HOST -u GH_CONFIG_DIR mise --no-config --no-env --no-hooks exec rust@1.98.1 mr-boxington@1.21.1 -- mbx build --release --locked --package velnor-actions-cli --bin velnor-actions\ncp target/release/velnor-actions {asset}\ntest -s {asset}"
    )
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

fn candidate_provenance_script(product: ProductAsset) -> String {
    let digest = archive::sidecar_digest_command(product.sidecar, product.binary);
    format!(
        "set -eu\ndigest=\"$({digest})\"\njq -n --arg repository \"$GITHUB_REPOSITORY\" --arg commit \"$GITHUB_SHA\" --arg target \"{}\" --arg asset \"{}\" --arg sha256 \"$digest\" --arg rust 1.98.1 --arg mr_boxington 1.21.1 '{{\"schema\":1,\"version\":\"{VERSION}\",\"repository\":$repository,\"commit\":$commit,\"target\":$target,\"asset\":$asset,\"sha256\":$sha256,\"toolchain\":{{\"rust\":$rust,\"mr-boxington\":$mr_boxington}}}}' > {}\nchmod 755 {}\nchmod 644 {} {}\ntest -s {}\ntest -s {}",
        product.target,
        product.binary,
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

/// Download the exact build artifact archive and restore its executable mode.
pub(super) fn download_steps(asset: ProductAsset, name: &str) -> Vec<Yaml> {
    download_steps_with(
        asset,
        workflow_steps::download_step(name, asset.workflow_artifact, asset.directory),
    )
}

/// Download the exact immutable artifact produced by one build job.
pub(super) fn download_build_steps(asset: ProductAsset, name: &str) -> Vec<Yaml> {
    let artifact_id = "${{ inputs.artifact_id }}";
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
pub(super) fn verify_provenance_script(asset: ProductAsset) -> String {
    verify_provenance_in_directory(asset, asset.directory)
}

fn verify_provenance_in_directory(asset: ProductAsset, directory: &str) -> String {
    let digest =
        archive::sidecar_digest_command(&format!("{directory}/{}", asset.sidecar), asset.binary);
    format!(
        "set -eu\ndigest=\"$({digest})\"\njq -e --arg version \"{VERSION}\" --arg repository \"$GITHUB_REPOSITORY\" --arg commit \"$GITHUB_SHA\" --arg target \"{}\" --arg asset \"{}\" --arg sha256 \"$digest\" --arg rust 1.98.1 --arg mr_boxington 1.21.1 '.schema == 1 and .version == $version and .repository == $repository and .commit == $commit and .target == $target and .asset == $asset and .sha256 == $sha256 and .toolchain.rust == $rust and .toolchain[\"mr-boxington\"] == $mr_boxington' {}/{} > /dev/null",
        asset.target, asset.binary, directory, asset.provenance
    )
}

/// Check candidate identity and deterministic generation on the uploaded bytes.
pub(super) fn qualification_script(binary: &str, directory: &str) -> String {
    format!(
        "set -eu\nunset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN GH_TOKEN MISE_GITHUB_TOKEN GH_HOST GH_CONFIG_DIR\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\ntest \"$(./{directory}/{binary} --version)\" = \"velnor-actions {VERSION}\"\nscripts/capture-opentofu-goldens.sh check-release \"$GITHUB_WORKSPACE/{directory}/{binary}\""
    )
}

/// Check the dispatched commit and required CI before building candidates.
pub(super) fn source_gate_job(hosted: Yaml) -> (String, Yaml) {
    finish(
        "verify-release-source",
        with_permissions(
            base("Verify generator release source", hosted, 20),
            workflow_steps::perm(&[("actions", "read"), ("contents", "read")]),
        ),
        vec![
            checkout_step(),
            workflow_steps::mise_step(),
            workflow_steps::bash_step(
                "Install pinned release gate tools",
                &format!(
                    "mise --no-config --no-env --no-hooks install rust@1.98.1 gh@{GH_VERSION} actionlint@1.7.12 shellcheck@0.11.0 zizmor@1.30.1"
                ),
            ),
            workflow_steps::bash_step(
                "Run actionlint",
                "mise --no-config --no-env --no-hooks exec actionlint@1.7.12 shellcheck@0.11.0 -- actionlint -color",
            ),
            workflow_steps::bash_step(
                "Run zizmor",
                "mise --no-config --no-env --no-hooks exec zizmor@1.30.1 -- zizmor --no-online-audits --config .zizmor.yml .github/workflows",
            ),
            ci_check_step(),
            workflow_steps::bash_step(
                "Check release freshness gate",
                "bash scripts/check-freshness.sh",
            ),
            workflow_steps::bash_step_with_token(
                "Recheck default-branch source",
                &format!(
                    "set -eu\ntest \"$GITHUB_REF\" = \"refs/heads/main\"\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\ntest \"$(mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh api repos/{REPOSITORY}/commits/main --jq .sha)\" = \"$GITHUB_SHA\""
                ),
            ),
        ],
    )
}

fn ci_check_step() -> Yaml {
    workflow_steps::bash_step_with_token(
        "Require successful CI at exact main SHA",
        &ci_check_script(),
    )
}

fn ci_check_script() -> String {
    format!(
        r#"set -eu
test "$GITHUB_REPOSITORY" = '{REPOSITORY}'
test "$GITHUB_REF" = 'refs/heads/main'
test "$GITHUB_EVENT_NAME" = 'workflow_dispatch'
test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
test "$(mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh api repos/{REPOSITORY}/commits/main --jq .sha)" = "$GITHUB_SHA"
runs="$(mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh api --paginate --slurp "repos/{REPOSITORY}/actions/workflows/{CI_WORKFLOW}/runs?head_sha=$GITHUB_SHA&branch=main&event=push&per_page=100")"
run="$(printf '%s\n' "$runs" | jq -ce --arg sha "$GITHUB_SHA" --arg repo '{REPOSITORY}' '[.[] | (.workflow_runs // [])[] | select(.path == ".github/workflows/{CI_WORKFLOW}" and .head_sha == $sha and .head_branch == "main" and .head_repository.full_name == $repo and .event == "push")] | sort_by([.run_number, .id]) | last // error("no exact-source main CI run")')"
test "$(printf '%s\n' "$run" | jq -r .status)" = completed
test "$(printf '%s\n' "$run" | jq -r .conclusion)" = success
run_id="$(printf '%s\n' "$run" | jq -r .id)"
attempt="$(printf '%s\n' "$run" | jq -r .run_attempt)"
jobs="$(mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh api --paginate --slurp "repos/{REPOSITORY}/actions/runs/$run_id/attempts/$attempt/jobs?per_page=100")"
printf '%s\n' "$jobs" | jq -e --arg sha "$GITHUB_SHA" '[.[] | (.jobs // [])[] | select(.name == "Required" and .head_sha == $sha and .head_branch == "main" and .status == "completed" and .conclusion == "success")] | length == 1' >/dev/null
test "$(mise --no-config --no-env --no-hooks exec gh@{GH_VERSION} -- gh api repos/{REPOSITORY}/commits/main --jq .sha)" = "$GITHUB_SHA""#
    )
}

#[cfg(test)]
#[path = "schema2_generator_release_assets_tests.rs"]
mod tests;
