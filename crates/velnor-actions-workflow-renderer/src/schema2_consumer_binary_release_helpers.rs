//! Validated workflow identity and reusable YAML step constructors.

use crate::steps::{CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;

use super::MiseSetup;
use super::scripts;
use super::{base, finish, run_step};

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
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
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

pub(super) fn upload_artifact_step(asset: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Upload binary release assets")),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "name".to_owned(),
                    Yaml::str("consumer-binary-${{ github.run_id }}-${{ github.run_attempt }}"),
                ),
                (
                    "path".to_owned(),
                    Yaml::str(format!(
                        "${{{{ runner.temp }}}}/consumer-binary-assets/{asset}\n${{{{ runner.temp }}}}/consumer-binary-assets/SHA256SUMS\n${{{{ runner.temp }}}}/consumer-binary-assets/release.json"
                    )),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(1)),
            ]),
        ),
    ])
}

pub(super) fn download_artifact_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Download binary release assets"),
        ),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "name".to_owned(),
                    Yaml::str("consumer-binary-${{ github.run_id }}-${{ github.run_attempt }}"),
                ),
                ("path".to_owned(), Yaml::str("assets")),
            ]),
        ),
    ])
}

pub(super) fn publish_job(
    spec: &super::ConsumerBinaryReleaseSpec,
    eligibility: &str,
    identity: &str,
    asset: &str,
) -> Result<(String, Yaml), crate::RenderError> {
    let fields = publish_fields();
    let steps = publish_steps(spec, eligibility, identity, asset)?;
    Ok(finish("publish-binary", fields, steps))
}

fn publish_fields() -> Vec<(String, Yaml)> {
    let mut fields = base(
        "Publish immutable Cargo version",
        Yaml::str("ubuntu-26.04"),
        30,
    );
    fields.push((
        "needs".to_owned(),
        needs(&["release-eligibility", "build-binary", "attest-binary"]),
    ));
    fields.push((
        "permissions".to_owned(),
        permissions(&[
            ("actions", "read"),
            ("attestations", "read"),
            ("contents", "write"),
        ]),
    ));
    fields.push((
        "environment".to_owned(),
        Yaml::Map(vec![(
            "name".to_owned(),
            Yaml::str("consumer-binary-release"),
        )]),
    ));
    fields
}

fn publish_steps(
    spec: &super::ConsumerBinaryReleaseSpec,
    eligibility: &str,
    identity: &str,
    asset: &str,
) -> Result<Vec<Yaml>, crate::RenderError> {
    let publish = scripts::publish(
        eligibility,
        identity,
        velnor_actions_contract::config::CONSUMER_BINARY_TARGET,
        asset,
    );
    Ok(vec![
        checkout_step("${{ needs.release-eligibility.outputs.source_sha }}"),
        download_artifact_step(),
        mise_step(&spec.linux_setup),
        run_step(
            "Install pinned Rust and GitHub CLI",
            &crate::commands::join_argv_for_run(&spec.install_tools_argv)?,
        ),
        script_step(
            "Verify provenance and publish once",
            "publish",
            &publish,
            publish_environment(spec),
        ),
    ])
}

fn publish_environment(spec: &super::ConsumerBinaryReleaseSpec) -> Vec<(&'static str, &str)> {
    vec![
        ("GH_TOKEN", "${{ github.token }}"),
        (
            "IMMUTABILITY_READ_TOKEN",
            "${{ secrets.IMMUTABILITY_READ_TOKEN }}",
        ),
        ("MANIFEST_PATH", &spec.manifest_path),
        ("PACKAGE_NAME", &spec.package),
        ("BINARY_NAME", &spec.bin),
        (
            "TARGET_TRIPLE",
            velnor_actions_contract::config::CONSUMER_BINARY_TARGET,
        ),
        ("DEFAULT_BRANCH", &spec.default_branch),
        (
            "EXPECTED_SOURCE_SHA",
            "${{ needs.release-eligibility.outputs.source_sha }}",
        ),
        (
            "EXPECTED_AUTHORITY_SHA",
            "${{ needs.release-eligibility.outputs.workflow_authority_sha }}",
        ),
        (
            "EXPECTED_CI_RUN_ID",
            "${{ needs.release-eligibility.outputs.ci_run_id }}",
        ),
        (
            "EXPECTED_CI_ATTEMPT",
            "${{ needs.release-eligibility.outputs.ci_attempt }}",
        ),
        (
            "EXPECTED_VERSION",
            "${{ needs.release-eligibility.outputs.package_version }}",
        ),
        (
            "EXPECTED_TAG",
            "${{ needs.release-eligibility.outputs.tag }}",
        ),
    ]
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
