//! Hosted image and macOS binary release workflows.
//!
//! `id-token` covers the whole job, so attest and GitHub-release upload are
//! different jobs. The attest job never receives `contents: write` or
//! `packages: write`. Checksums are `sha256sum` / `shasum` of the bytes just
//! built. Dispatch has no inputs, so a caller cannot supply a shell fragment
//! or a checksum.

use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

use super::features::{CHECKOUT_USES, base, finish, publish_step, run_step};
use super::release_eligibility;

/// Same `jdx/mise-action` commit CI pins. Not a floating tag.
const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
/// Catalog version. The Linux cache checksum is not reused on macOS.
const MISE_VERSION: &str = "2026.9.18";
/// `actions/attest-build-provenance` tag `v4.2.2` (commit, not a floating tag).
const ATTEST_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";
const ASSET_DIR: &str = "assets";

pub(super) fn build_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    timeout: i64,
    mut steps: Vec<Yaml>,
    upload_name: &str,
    files: &[&str],
) -> (String, Yaml) {
    let mut prefixed = vec![checkout_step(
        "${{ needs['release-eligibility'].outputs.source_sha }}",
    )];
    prefixed.append(&mut steps);
    prefixed.push(upload_step(upload_name, artifact_name(id), files));
    finish(
        id,
        with_needs(
            with_permissions(base(name, runs_on, timeout), build_permissions()),
            &[release_eligibility::JOB_ID],
        ),
        prefixed,
    )
}

pub(super) fn attest_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    needs: &[&str],
    artifact: &str,
    files: &[&str],
) -> (String, Yaml) {
    finish(
        id,
        with_needs(
            with_permissions(base(name, runs_on, 20), attest_permissions()),
            needs,
        ),
        vec![download_step(artifact), attest_step(&subject_list(files))],
    )
}

pub(super) struct Publish<'a> {
    pub(super) id: &'a str,
    pub(super) name: &'a str,
    pub(super) needs: &'a [&'a str],
    pub(super) artifact: &'a str,
    pub(super) prefix: &'a str,
    pub(super) notes: &'a str,
    pub(super) files: &'a [&'a str],
    pub(super) workflow_path: &'a str,
}

pub(super) fn publish_job(runs_on: Yaml, spec: &Publish<'_>) -> (String, Yaml) {
    finish(
        spec.id,
        with_needs(
            with_permissions(base(spec.name, runs_on, 30), publish_permissions()),
            spec.needs,
        ),
        vec![
            checkout_step("${{ needs['release-eligibility'].outputs.source_sha }}"),
            release_eligibility::mise_step(),
            run_step(
                "Install pinned GitHub CLI",
                &format!(
                    "mise --no-config --no-env --no-hooks install gh@{}",
                    release_eligibility::GH_VERSION
                ),
            ),
            download_step(spec.artifact),
            release_eligibility::check_step(spec.workflow_path),
            publish_step(&release_command(spec.prefix, spec.notes, spec.files)),
        ],
    )
}

fn artifact_name(build_id: &str) -> &'static str {
    if build_id.ends_with("images") {
        "image-assets"
    } else {
        "binary-assets"
    }
}

pub(super) fn mise_step() -> Yaml {
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

fn checkout_step(ref_value: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("1")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
                ("ref".to_owned(), Yaml::str(ref_value)),
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

fn download_step(artifact: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Download built assets")),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str(ASSET_DIR)),
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
        Yaml::Seq(needs.iter().map(|need| Yaml::str(*need)).collect()),
    ));
    fields
}

/// Build uploads a workflow artifact. That needs `actions: write`, not contents write.
fn build_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "read")])
}

/// Attest job: `id-token`, no `contents: write`. Not a registry upload.
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

fn release_command(prefix: &str, notes: &str, files: &[&str]) -> String {
    format!(
        "set -eu\ncd {ASSET_DIR}\ntag=\"{prefix}-${{GITHUB_SHA}}\"\ngh release create \"$tag\" -R \"${{GITHUB_REPOSITORY}}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"{notes}\" {}",
        files.join(" ")
    )
}

pub(super) fn document(name: &str, jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "workflow_dispatch".to_owned(),
                Yaml::Map(vec![(
                    "inputs".to_owned(),
                    Yaml::Map(vec![(
                        "source_sha".to_owned(),
                        Yaml::Map(vec![
                            (
                                "description".to_owned(),
                                Yaml::str("Exact tested main commit to build and publish"),
                            ),
                            ("required".to_owned(), Yaml::Bool(true)),
                            ("type".to_owned(), Yaml::str("string")),
                        ]),
                    )]),
                )]),
            )]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}
