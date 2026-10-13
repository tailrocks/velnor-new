//! YAML step helpers for the generic Rust binary-release workflow.

use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

use crate::setup::MiseSetup;

pub(super) fn job_base(
    name: &str,
    runner: &str,
    timeout: i64,
    permissions: &[(&str, &str)],
    condition: &str,
) -> Vec<(String, Yaml)> {
    vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), Yaml::str(runner)),
        ("timeout-minutes".to_owned(), Yaml::Int(timeout)),
        ("permissions".to_owned(), permission_map(permissions)),
        ("if".to_owned(), Yaml::str(condition)),
    ]
}

pub(super) fn checkout_step(uses: &str, reference: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out exact source")),
        ("uses".to_owned(), Yaml::str(uses)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("0")),
                ("persist-credentials".to_owned(), Yaml::Bool(false)),
                ("ref".to_owned(), Yaml::str(reference)),
                ("fetch-tags".to_owned(), Yaml::Bool(true)),
            ]),
        ),
    ])
}

pub(super) fn mise_setup_step(setup: &MiseSetup) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Set up pinned Mise")),
        ("uses".to_owned(), Yaml::str(setup.uses.clone())),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("version".to_owned(), Yaml::str(setup.version.clone())),
                ("sha256".to_owned(), Yaml::str(setup.sha256.clone())),
                ("install".to_owned(), Yaml::Bool(false)),
                ("env".to_owned(), Yaml::Bool(false)),
                ("cache".to_owned(), Yaml::Bool(false)),
                ("cache_save".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

pub(super) fn run_step(name: &str, run: &str) -> Yaml {
    run_step_with(name, "", run, &[])
}

pub(super) fn run_step_with(name: &str, id: &str, run: &str, env: &[(&str, &str)]) -> Yaml {
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if !id.is_empty() {
        fields.push(("id".to_owned(), Yaml::str(id)));
    }
    fields.push(("shell".to_owned(), Yaml::str("bash")));
    fields.push(("run".to_owned(), Yaml::str(run)));
    if !env.is_empty() {
        fields.push((
            "env".to_owned(),
            Yaml::Map(
                env.iter()
                    .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
                    .collect(),
            ),
        ));
    }
    Yaml::Map(fields)
}

pub(super) fn upload_step(name: &str, path: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Upload binary artifact")),
        ("id".to_owned(), Yaml::str("upload")),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("name".to_owned(), Yaml::str(name)),
                ("path".to_owned(), Yaml::str(path)),
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
                ("artifact-ids".to_owned(), Yaml::str(artifact)),
                ("digest-mismatch".to_owned(), Yaml::str("error")),
                ("github-token".to_owned(), Yaml::str("${{ github.token }}")),
                ("path".to_owned(), Yaml::str(path)),
                ("run-id".to_owned(), Yaml::str("${{ github.run_id }}")),
            ]),
        ),
    ])
}

pub(super) fn permission_map(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}
