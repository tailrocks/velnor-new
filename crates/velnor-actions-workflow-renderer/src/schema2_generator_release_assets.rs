//! Release candidate builds, target assets, and qualification.

use crate::yaml::Yaml;

use super::archive;
use super::finish;
use super::workflow_steps::{self, checkout_step, with_permissions};

pub(super) const VERSION: &str = "0.1.1";
pub(super) const REPOSITORY: &str = "tailrocks/velnor-new";
const CI_WORKFLOW: &str = "ci.yml";
const GH_VERSION: &str = "2.102.0";
pub(super) const QUALIFICATION_TOOLS_INSTALL: &str = "\
set -eu
mise --no-config --no-env --no-hooks install actionlint@1.7.12 shellcheck@0.11.0 zizmor@1.30.1";

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
    pub attest_job: &'static str,
    pub checksum_command: &'static str,
}

/// Linux x86_64 binary.
pub(super) const LINUX: ProductAsset = ProductAsset {
    target: "x86_64-unknown-linux-gnu",
    binary: "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
    sidecar: "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.sha256",
    provenance: "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.provenance.json",
    archive: "generator-linux-assets.tar",
    workflow_artifact: "generator-linux-assets",
    directory: "linux-assets",
    attest_job: "attest-linux",
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
    attest_job: "attest-macos",
    checksum_command: "shasum -a 256 --check",
};

/// macOS x86_64 binary.
pub(super) const MACOS_X86_64: ProductAsset = ProductAsset {
    target: "x86_64-apple-darwin",
    binary: "velnor-actions-0.1.1-x86_64-apple-darwin",
    sidecar: "velnor-actions-0.1.1-x86_64-apple-darwin.sha256",
    provenance: "velnor-actions-0.1.1-x86_64-apple-darwin.provenance.json",
    archive: "generator-macos-intel-assets.tar",
    workflow_artifact: "generator-macos-intel-assets",
    directory: "macos-intel-assets",
    attest_job: "attest-macos-intel",
    checksum_command: "shasum -a 256 --check",
};

pub(super) const ASSETS: [ProductAsset; 3] = [LINUX, MACOS_ARM64, MACOS_X86_64];

/// Build, inspect, and checksum one native candidate before uploading it.
pub(super) fn build_steps(
    asset: &str,
    verify_name: &str,
    verify: &str,
    sum_cmd: &str,
    sidecar: &str,
    provenance: &str,
    target: &str,
    archive: &str,
) -> Vec<Yaml> {
    vec![
        workflow_steps::mise_step(),
        workflow_steps::bash_step(
            "Install pinned Rust and MBX",
            "mise --no-config --no-env --no-hooks install rust@1.98.1 mr-boxington@1.21.1",
        ),
        workflow_steps::bash_step("Build velnor-actions with MBX", &build_script(asset)),
        workflow_steps::bash_step(verify_name, verify),
        workflow_steps::bash_step("Checksum built bytes", &sum_script(sum_cmd, asset, sidecar)),
        workflow_steps::bash_step(
            "Record candidate provenance",
            &candidate_provenance_script(asset, sidecar, provenance, target),
        ),
        workflow_steps::bash_step(
            "Package candidate preserving executable mode",
            &archive_script(asset, sidecar, provenance, archive),
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

fn candidate_provenance_script(
    asset: &str,
    sidecar: &str,
    provenance: &str,
    target: &str,
) -> String {
    let digest = archive::sidecar_digest_command(sidecar, asset);
    format!(
        "set -eu\ndigest=\"$({digest})\"\njq -n --arg repository \"$GITHUB_REPOSITORY\" --arg commit \"$GITHUB_SHA\" --arg target \"{target}\" --arg asset \"{asset}\" --arg sha256 \"$digest\" --arg rust 1.98.1 --arg mr_boxington 1.21.1 '{{\"schema\":1,\"version\":\"{VERSION}\",\"repository\":$repository,\"commit\":$commit,\"target\":$target,\"asset\":$asset,\"sha256\":$sha256,\"toolchain\":{{\"rust\":$rust,\"mr-boxington\":$mr_boxington}}}}' > {provenance}\nchmod 755 {asset}\nchmod 644 {sidecar} {provenance}\ntest -s {provenance}\ntest -s {asset}"
    )
}

fn archive_script(asset: &str, sidecar: &str, provenance: &str, archive: &str) -> String {
    format!("set -eu\ntar -cf {archive} {asset} {sidecar} {provenance}\ntest -s {archive}")
}

/// Download the exact build artifact archive and restore its executable mode.
pub(super) fn download_steps(asset: ProductAsset, name: &str) -> Vec<Yaml> {
    vec![
        download_step(name, asset),
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

fn download_step(name: &str, asset: ProductAsset) -> Yaml {
    workflow_steps::download_step(name, asset.workflow_artifact, asset.directory)
}

/// Check candidate identity and deterministic generation on the uploaded bytes.
pub(super) fn qualification_script(binary: &str, directory: &str) -> String {
    format!(
        "set -eu\nunset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN GH_TOKEN MISE_GITHUB_TOKEN GH_HOST GH_CONFIG_DIR\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\ntest \"$(./{directory}/{binary} --version)\" = \"velnor-actions {VERSION}\"\nscripts/capture-opentofu-goldens.sh check-release \"$GITHUB_WORKSPACE/{directory}/{binary}\"\nmise --no-config --no-env --no-hooks exec actionlint@1.7.12 shellcheck@0.11.0 -- actionlint -color\nmise --no-config --no-env --no-hooks exec zizmor@1.30.1 -- zizmor --no-online-audits --config .zizmor.yml .github/workflows"
    )
}

/// Check the dispatched commit and required CI before building candidates.
pub(super) fn source_gate_job(hosted: Yaml) -> (String, Yaml) {
    finish(
        "verify-release-source",
        with_permissions(
            super::base("Verify generator release source", hosted, 20),
            workflow_steps::perm(&[("actions", "read"), ("contents", "read")]),
        ),
        vec![
            checkout_step(),
            workflow_steps::mise_step(),
            workflow_steps::bash_step(
                "Install pinned release gate tools",
                &format!(
                    "mise --no-config --no-env --no-hooks install rust@1.98.1 gh@{GH_VERSION}"
                ),
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

#[cfg(test)]
mod tests {
    use super::{LINUX, qualification_script, verify_provenance_in_directory};
    use std::error::Error;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    #[test]
    fn wrong_checksum_filename_stops_before_candidate_execution() -> Result<(), Box<dyn Error>> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory_name = format!("target/release-sidecar-test-{}-{nonce}", std::process::id());
        let directory = root.join(&directory_name);
        fs::create_dir_all(&directory)?;
        let scratch = Scratch(directory.clone());
        let executed = scratch.0.join("candidate-executed");
        let candidate = directory.join(LINUX.binary);
        fs::write(
            &candidate,
            "#!/bin/sh\nprintf '%s\\n' executed >> \"$CANDIDATE_EXECUTED\"\nprintf '%s\\n' 'velnor-actions 0.1.1'\n",
        )?;
        let mut permissions = fs::metadata(&candidate)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&candidate, permissions)?;
        fs::write(
            directory.join(LINUX.sidecar),
            format!("{}  unrelated-binary\n", "a".repeat(64)),
        )?;
        let sha = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&root)
            .output()?;
        if !sha.status.success() {
            return Err("cannot read candidate source SHA".into());
        }
        let source_sha = String::from_utf8(sha.stdout)?.trim().to_owned();
        let command = format!(
            "{}\n{}",
            verify_provenance_in_directory(LINUX, &directory_name),
            qualification_script(LINUX.binary, &directory_name)
        );
        let status = Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(&root)
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_SHA", source_sha)
            .env("GITHUB_WORKSPACE", &root)
            .env("CANDIDATE_EXECUTED", &executed)
            .status()?;
        assert!(!status.success(), "accepted a sidecar for another filename");
        assert!(
            !executed.exists(),
            "candidate ran before its sidecar passed validation"
        );
        Ok(())
    }
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
