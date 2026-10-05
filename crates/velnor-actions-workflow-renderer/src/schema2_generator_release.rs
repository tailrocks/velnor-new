//! Generator release: Linux x64 and macOS arm64 `velnor-actions` assets.
//!
//! The tag is `generator-<sha>` with `--latest=false`. It does not move
//! `v0.1.0`. Attest jobs never receive `contents: write`. Only publish does.

use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

use super::Schema2WorkflowRequest;
use super::features::{CHECKOUT_USES, base, finish, publish_step, run_step};

/// GitHub-hosted macOS label. The arm64 binary is not built on Ubuntu.
const MACOS_RUNS_ON: &str = "macos-15";
/// Same `jdx/mise-action` commit CI pins. Not a floating tag.
const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
/// Catalog version. Not a floating `latest`.
const MISE_VERSION: &str = "2026.9.18";
/// `actions/attest-build-provenance` tag `v4.2.2` (commit, not a floating tag).
const ATTEST_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";
const LINUX_BIN: &str = "velnor-actions-0.1.0-x86_64-unknown-linux-gnu";
const MACOS_BIN: &str = "velnor-actions-0.1.0-aarch64-apple-darwin";
const LINUX_SUM: &str = "velnor-actions-0.1.0-x86_64-unknown-linux-gnu.sha256";
const MACOS_SUM: &str = "velnor-actions-0.1.0-aarch64-apple-darwin.sha256";
const LINUX_ARTIFACT: &str = "generator-linux-assets";
const MACOS_ARTIFACT: &str = "generator-macos-assets";
const LINUX_DIR: &str = "linux-assets";
const MACOS_DIR: &str = "macos-assets";
const ASSET_DIR: &str = "assets";

const RUST_INSTALL: &str = "\
set -eu
mise --no-config --no-env --no-hooks install rust@1.98.1";

/// Linux x64 and macOS arm64 builds, two attestations, then one publish.
///
/// # Errors
///
/// An illegal hosted or macOS label fails.
pub(super) fn generator_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let macos = runs_on_yaml(MACOS_RUNS_ON)?;
    let mut jobs = linux_jobs(hosted.clone());
    jobs.extend(macos_jobs(macos));
    jobs.push(publish_job(hosted));
    Ok(document(jobs))
}

fn linux_jobs(hosted: Yaml) -> Vec<(String, Yaml)> {
    let files = [LINUX_BIN, LINUX_SUM];
    let steps = build_steps(
        LINUX_BIN,
        "Verify ELF architecture",
        &linux_verify(LINUX_BIN),
        "sha256sum",
        LINUX_SUM,
    );
    vec![
        build_job(
            "build-linux",
            "Build Linux velnor-actions",
            hosted.clone(),
            steps,
            "Upload Linux assets",
            LINUX_ARTIFACT,
            &files,
        ),
        attest_job(
            "attest-linux",
            "Attest Linux velnor-actions",
            hosted,
            "build-linux",
            LINUX_ARTIFACT,
            &files,
        ),
    ]
}

fn macos_jobs(macos: Yaml) -> Vec<(String, Yaml)> {
    let files = [MACOS_BIN, MACOS_SUM];
    let steps = build_steps(
        MACOS_BIN,
        "Verify Mach-O architecture",
        &macos_verify(MACOS_BIN),
        "shasum -a 256",
        MACOS_SUM,
    );
    vec![
        build_job(
            "build-macos",
            "Build macOS velnor-actions",
            macos.clone(),
            steps,
            "Upload macOS assets",
            MACOS_ARTIFACT,
            &files,
        ),
        attest_job(
            "attest-macos",
            "Attest macOS velnor-actions",
            macos,
            "build-macos",
            MACOS_ARTIFACT,
            &files,
        ),
    ]
}

fn build_steps(
    asset: &str,
    verify_name: &str,
    verify: &str,
    sum_cmd: &str,
    sidecar: &str,
) -> Vec<Yaml> {
    vec![
        mise_step(),
        run_step("Install pinned Rust", RUST_INSTALL),
        run_step("Build velnor-actions", &build_script(asset)),
        run_step(verify_name, verify),
        run_step("Checksum built bytes", &sum_script(sum_cmd, asset, sidecar)),
    ]
}

fn build_script(asset: &str) -> String {
    format!(
        "set -eu\nmise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo build --locked --release -p velnor-actions-cli\ncp target/release/velnor-actions {asset}\ntest -s {asset}"
    )
}

fn linux_verify(asset: &str) -> String {
    format!(
        "set -eu\ndesc=\"$(file -b {asset})\"\ncase \"$desc\" in\n  *ELF*x86-64*) ;;\n  *) echo \"not an x86-64 ELF: $desc\" >&2; exit 1 ;;\nesac"
    )
}

fn macos_verify(asset: &str) -> String {
    format!(
        "set -eu\ndesc=\"$(file -b {asset})\"\ncase \"$desc\" in\n  *Mach-O*arm64*) ;;\n  *) echo \"not an arm64 Mach-O: $desc\" >&2; exit 1 ;;\nesac"
    )
}

fn sum_script(command: &str, asset: &str, sidecar: &str) -> String {
    format!("set -eu\n{command} {asset} > {sidecar}")
}

fn publish_script() -> String {
    format!(
        "set -eu\ntag=\"generator-${{GITHUB_SHA}}\"\ngh release create \"$tag\" -R \"${{GITHUB_REPOSITORY}}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"velnor-actions 0.1.0 built from ${{GITHUB_SHA}}.\" {LINUX_DIR}/{LINUX_BIN} {LINUX_DIR}/{LINUX_SUM} {MACOS_DIR}/{MACOS_BIN} {MACOS_DIR}/{MACOS_SUM}"
    )
}

fn build_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    steps: Vec<Yaml>,
    upload_name: &str,
    artifact: &str,
    files: &[&str],
) -> (String, Yaml) {
    let mut prefixed = vec![checkout_step()];
    prefixed.extend(steps);
    prefixed.push(upload_step(upload_name, artifact, files));
    finish(
        id,
        with_permissions(base(name, runs_on, 120), build_permissions()),
        prefixed,
    )
}

fn attest_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    needs: &str,
    artifact: &str,
    files: &[&str],
) -> (String, Yaml) {
    finish(
        id,
        with_needs(
            with_permissions(base(name, runs_on, 20), attest_permissions()),
            &[needs],
        ),
        vec![
            download_step("Download built assets", artifact, ASSET_DIR),
            attest_step(&subject_list(files)),
        ],
    )
}

fn publish_job(hosted: Yaml) -> (String, Yaml) {
    finish(
        "publish-generator",
        with_needs(
            with_permissions(
                base("Publish velnor-actions", hosted, 30),
                publish_permissions(),
            ),
            &["attest-linux", "attest-macos"],
        ),
        vec![
            checkout_step(),
            download_step("Download Linux assets", LINUX_ARTIFACT, LINUX_DIR),
            download_step("Download macOS assets", MACOS_ARTIFACT, MACOS_DIR),
            publish_step(&publish_script()),
        ],
    )
}

fn mise_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Setup Mise")),
        ("uses".to_owned(), Yaml::str(MISE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("cache".to_owned(), Yaml::str("false")),
                ("env".to_owned(), Yaml::str("false")),
                ("install".to_owned(), Yaml::str("false")),
                ("version".to_owned(), Yaml::str(MISE_VERSION)),
            ]),
        ),
    ])
}

fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("1")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
            ]),
        ),
    ])
}

fn upload_step(name: &str, artifact: &str, files: &[&str]) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str(newline_list(files))),
                ("retention-days".to_owned(), Yaml::Int(1)),
            ]),
        ),
    ])
}

fn download_step(name: &str, artifact: &str, path: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str(path)),
            ]),
        ),
    ])
}

fn attest_step(subjects: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest built artifacts")),
        ("uses".to_owned(), Yaml::str(ATTEST_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![("subject-path".to_owned(), Yaml::str(subjects))]),
        ),
    ])
}

fn with_permissions(mut fields: Vec<(String, Yaml)>, perms: Yaml) -> Vec<(String, Yaml)> {
    fields.push(("permissions".to_owned(), perms));
    fields
}

fn with_needs(mut fields: Vec<(String, Yaml)>, needs: &[&str]) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(needs.iter().copied().map(Yaml::str).collect()),
    ));
    fields
}

/// Build uploads a workflow artifact. That needs `actions: write`, not contents write.
fn build_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "read")])
}

/// Attest job: `id-token`, no `contents: write`.
fn attest_permissions() -> Yaml {
    perm(&[
        ("actions", "read"),
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

/// Only the release-upload job may write repository contents.
fn publish_permissions() -> Yaml {
    perm(&[("actions", "read"), ("contents", "write")])
}

fn perm(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

fn subject_list(files: &[&str]) -> String {
    files
        .iter()
        .map(|file| format!("{ASSET_DIR}/{file}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn newline_list(files: &[&str]) -> String {
    files.join("\n")
}

fn document(jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Generator release")),
        (
            "on".to_owned(),
            Yaml::Map(vec![("workflow_dispatch".to_owned(), Yaml::Map(vec![]))]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}
