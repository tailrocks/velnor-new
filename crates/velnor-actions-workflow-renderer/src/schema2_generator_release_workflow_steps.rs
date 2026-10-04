//! Reusable workflow steps and permission sets for the generator release.

use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

use super::super::features::CHECKOUT_USES;

const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
const MISE_VERSION: &str = "2026.9.18";
const ATTEST_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";
pub(super) fn bash_step(name: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

/// Run one API step with only the repository's read-scoped token.
pub(super) fn bash_step_with_token(name: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ github.token }}"),
            )]),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

pub(super) fn publish_step(run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Publish GitHub release")),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ github.token }}"),
            )]),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
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

pub(super) fn checkout_step() -> Yaml {
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

pub(super) fn upload_step(name: &str, artifact: &str, files: &[&str]) -> Yaml {
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

pub(super) fn download_step(name: &str, artifact: &str, path: &str) -> Yaml {
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

pub(super) fn attest_step(subjects: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest built artifacts")),
        ("uses".to_owned(), Yaml::str(ATTEST_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![("subject-path".to_owned(), Yaml::str(subjects))]),
        ),
    ])
}

pub(super) fn with_permissions(
    mut fields: Vec<(String, Yaml)>,
    permissions: Yaml,
) -> Vec<(String, Yaml)> {
    fields.push(("permissions".to_owned(), permissions));
    fields
}

pub(super) fn with_needs(mut fields: Vec<(String, Yaml)>, needs: &[&str]) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(needs.iter().copied().map(Yaml::str).collect()),
    ));
    fields
}

/// Build uploads a workflow artifact. That needs `actions: write`, not contents write.
pub(super) fn build_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "read")])
}

/// Attestation uploads bundle artifacts but cannot write repository contents.
pub(super) fn attest_permissions() -> Yaml {
    perm(&[
        ("actions", "write"),
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

/// Only the release-upload job may write repository contents.
pub(super) fn publish_permissions() -> Yaml {
    perm(&[("actions", "read"), ("contents", "write")])
}

pub(super) fn perm(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

pub(super) fn subject_list(files: &[&str], directory: &str) -> String {
    files
        .iter()
        .map(|file| format!("{directory}/{file}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn newline_list(files: &[&str]) -> String {
    files.join("\n")
}
