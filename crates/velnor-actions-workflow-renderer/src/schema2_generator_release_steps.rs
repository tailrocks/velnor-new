//! GitHub Actions step and permission constructors for the release workflow.

use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

pub(super) fn mise_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Setup Mise")),
        ("uses".to_owned(), Yaml::str(super::MISE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("cache".to_owned(), Yaml::str("false")),
                ("env".to_owned(), Yaml::str("false")),
                ("install".to_owned(), Yaml::str("false")),
                ("version".to_owned(), Yaml::str(super::MISE_VERSION)),
            ]),
        ),
    ])
}

pub(super) fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out release SHA")),
        ("uses".to_owned(), Yaml::str(super::CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("1")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
                ("ref".to_owned(), Yaml::str("${{ github.sha }}")),
            ]),
        ),
    ])
}

pub(super) fn checkout_candidate_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Check out pull-request merge SHA"),
        ),
        ("uses".to_owned(), Yaml::str(super::CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::Int(1)),
                ("persist-credentials".to_owned(), Yaml::Bool(false)),
                ("ref".to_owned(), Yaml::str("${{ github.sha }}")),
            ]),
        ),
    ])
}

pub(super) fn upload_step(name: &str, artifact: &str, files: &[&str]) -> Yaml {
    upload_step_inner(None, name, artifact, files)
}

pub(super) fn upload_step_with_id(id: &str, name: &str, artifact: &str, files: &[&str]) -> Yaml {
    upload_step_inner(Some(id), name, artifact, files)
}

fn upload_step_inner(id: Option<&str>, name: &str, artifact: &str, files: &[&str]) -> Yaml {
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if let Some(id) = id {
        fields.push(("id".to_owned(), Yaml::str(id)));
    }
    fields.push(("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)));
    fields.push((
        "with".to_owned(),
        Yaml::Map(vec![
            ("if-no-files-found".to_owned(), Yaml::str("error")),
            ("name".to_owned(), Yaml::str(artifact)),
            ("path".to_owned(), Yaml::str(newline_list(files))),
            ("retention-days".to_owned(), Yaml::Int(1)),
        ]),
    ));
    Yaml::Map(fields)
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

pub(super) fn download_artifact_id_step(name: &str, artifact_id: &str, path: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("artifact-ids".to_owned(), Yaml::str(artifact_id)),
                ("path".to_owned(), Yaml::str(path)),
            ]),
        ),
    ])
}

pub(super) fn attest_step(subjects: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest built assets")),
        ("uses".to_owned(), Yaml::str(super::ATTEST_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![("subject-path".to_owned(), Yaml::str(subjects))]),
        ),
    ])
}

pub(super) fn bash_run_step(name: &str, script: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(script)),
    ])
}

pub(super) fn bash_run_step_with_id(id: &str, name: &str, script: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(script)),
    ])
}

pub(super) fn bash_run_step_with_env(
    name: &str,
    script: &str,
    env_name: &str,
    env_value: &str,
) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        (
            "env".to_owned(),
            Yaml::Map(vec![(env_name.to_owned(), Yaml::str(env_value))]),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(script)),
    ])
}

pub(super) fn bash_run_step_with_id_env(
    id: &str,
    name: &str,
    script: &str,
    env_name: &str,
    env_value: &str,
) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        (
            "env".to_owned(),
            Yaml::Map(vec![(env_name.to_owned(), Yaml::str(env_value))]),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(script)),
    ])
}

pub(super) fn token_bash_run_step(name: &str, script: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ github.token }}"),
            )]),
        ),
        ("run".to_owned(), Yaml::str(script)),
    ])
}

pub(super) fn publish_bash_step(script: &str) -> Yaml {
    token_bash_run_step("Publish GitHub release", script)
}

pub(super) fn gate_permissions() -> Yaml {
    perm(&[("actions", "read"), ("contents", "read")])
}

pub(super) fn build_permissions() -> Yaml {
    perm(&[("contents", "read")])
}

pub(super) fn attest_permissions() -> Yaml {
    perm(&[
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

pub(super) fn manifest_permissions() -> Yaml {
    perm(&[
        ("actions", "read"),
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

pub(super) fn publish_permissions() -> Yaml {
    perm(&[
        ("actions", "read"),
        ("attestations", "read"),
        ("contents", "write"),
    ])
}

pub(super) fn subject_list(files: &[&str]) -> String {
    files
        .iter()
        .map(|file| format!("{}/{file}", super::ASSET_DIR))
        .collect::<Vec<_>>()
        .join("\n")
}

fn newline_list(files: &[&str]) -> String {
    files.join("\n")
}

fn perm(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}
