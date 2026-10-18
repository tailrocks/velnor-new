//! Shared exact stable release workflow YAML constructors.

use super::MiseSetup;
use crate::yaml::Yaml;

pub(super) fn valid_repository(repository: &str) -> bool {
    let Some((owner, name)) = repository.split_once('/') else {
        return false;
    };
    !name.contains('/')
        && [owner, name].into_iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
}

pub(super) fn checkout_step(ref_value: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out exact source")),
        ("uses".to_owned(), Yaml::str(crate::steps::CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::Int(1)),
                ("persist-credentials".to_owned(), Yaml::Bool(false)),
                ("ref".to_owned(), Yaml::str(ref_value)),
            ]),
        ),
    ])
}

pub(super) fn mise_step(setup: &MiseSetup) -> Yaml {
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

pub(super) fn script_step(name: &str, id: &str, script: &str, env: Vec<(&str, &str)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(
                env.into_iter()
                    .map(|(key, value)| (key.to_owned(), Yaml::str(value)))
                    .collect(),
            ),
        ),
        ("run".to_owned(), Yaml::str(script)),
    ])
}

pub(super) fn identity_env(spec: &super::ConsumerBinaryReleaseSpec) -> Vec<(&'static str, &str)> {
    vec![
        ("MANIFEST_PATH", &spec.manifest_path),
        ("PACKAGE_NAME", &spec.package),
        ("BINARY_NAME", &spec.bin),
        (
            "TARGET_TRIPLE",
            velnor_actions_contract::config::CONSUMER_BINARY_TARGET,
        ),
    ]
}

pub(super) fn upload_artifact_step(_asset: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Upload stable package output")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::UPLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "name".to_owned(),
                    Yaml::str(
                        "velnor-stable-producer-${{ github.run_id }}-${{ github.run_attempt }}",
                    ),
                ),
                (
                    "path".to_owned(),
                    Yaml::str("${{ runner.temp }}/velnor/preview-output"),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(30)),
                ("overwrite".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

pub(super) fn permissions(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

pub(super) fn outputs(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(format!("${{{{ {value} }}}}"))))
            .collect(),
    )
}

pub(super) fn needs(values: &[&str]) -> Yaml {
    Yaml::Seq(values.iter().map(|value| Yaml::str(*value)).collect())
}
