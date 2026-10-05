//! Release candidate builds, target assets, and qualification.

use velnor_actions_contract::ReleaseTarget;

use super::archive;
use super::workflow_steps;
use crate::yaml::Yaml;

pub(super) const VERSION: &str = "0.1.1";
pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";

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
    pub attest_job: &'static str,
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
    attest_job: "attest-linux",
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
    attest_job: "attest-macos",
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
    attest_job: "attest-macos-intel",
    upload_name: "Upload macOS x86_64 assets",
    sum_command: "shasum -a 256",
    checksum_command: "shasum -a 256 --check",
};

pub(super) const ASSETS: [ProductAsset; 3] = [LINUX, MACOS_ARM64, MACOS_X86_64];

/// Build, inspect, and checksum one native candidate before uploading it.
pub(super) fn build_steps(
    product: ProductAsset,
    verify_name: &str,
    verify: &str,
    pins: &crate::schema2::ProductReleasePins,
) -> Result<Vec<Yaml>, crate::RenderError> {
    let build = build_script(product.binary, &pins.build_argv)?;
    Ok(vec![
        workflow_steps::mise_step(pins.setup_for(product.target))?,
        workflow_steps::command_step(
            "Install pinned Rust and MBX",
            &pins.install_build_tools_argv,
        )?,
        workflow_steps::bash_step("Build velnor-actions with MBX", &build),
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
    ])
}

/// The build emits the filename recorded in the release manifest.
fn build_script(asset: &str, build_argv: &[String]) -> Result<String, crate::RenderError> {
    Ok(format!(
        "set -eu\nenv -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN -u MISE_GITHUB_TOKEN -u GH_TOKEN -u GH_HOST -u GH_CONFIG_DIR {}\ncp target/release/velnor-actions {asset}\ntest -s {asset}",
        crate::commands::join_argv_for_run(build_argv)?
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
    pins: &crate::schema2::ProductReleasePins,
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
    pins: &crate::schema2::ProductReleasePins,
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

#[cfg(test)]
#[path = "schema2_generator_release_assets_tests.rs"]
mod tests;
