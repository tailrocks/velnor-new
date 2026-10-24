use velnor_actions_contract::ReleaseTarget;

use crate::yaml::Yaml;

use super::{ProductReleasePins, generator_release};

const ARTIFACT_NAME: &str = "velnor-paired-sdk-${{ github.run_id }}-${{ github.run_attempt }}";
const ARTIFACT_DIR_SHELL: &str = "$RUNNER_TEMP/paired-sdk-artifact";
const ARTIFACT_DIR_ACTION: &str = "${{ runner.temp }}/paired-sdk-artifact";
const BINARY_NAME: &str = "velnor-testcontainers-tls-fixture";
const MANIFEST_NAME: &str = "manifest.txt";
const BUILDER_BINARY_SHA: &str = "${{ needs.paired-sdk-build.outputs.binary_sha256 }}";

pub(super) fn builder_steps(pins: &ProductReleasePins) -> Result<Vec<Yaml>, crate::RenderError> {
    let mut steps = vec![super::features::checkout_step()];
    steps.extend([
        generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?,
        generator_release::mise_install_step(
            "Install pinned Rust for the paired SDK fixture",
            &pins.install_runner_build_tools_argv,
        )?,
        generator_release::verify_rust_step(pins)?,
    ]);
    steps.extend(
        pins.qualify_mbx_action_steps
            .iter()
            .map(crate::steps_plain::plain_step_to_yaml)
            .collect::<Result<Vec<_>, _>>()?,
    );
    steps.push(build_digest_output_step());
    steps.push(upload_step());
    Ok(steps)
}

pub(super) fn consumer_steps() -> Vec<Yaml> {
    vec![
        super::features::checkout_step(),
        download_step(),
        Yaml::Map(vec![
            (
                "name".to_owned(),
                Yaml::str("Run Node and Rust HTTPS checks"),
            ),
            ("shell".to_owned(), Yaml::str("bash")),
            (
                "env".to_owned(),
                Yaml::Map(vec![(
                    "VELNOR_PAIRED_SDK_BINARY_SHA256".to_owned(),
                    Yaml::str(BUILDER_BINARY_SHA),
                )]),
            ),
            (
                "run".to_owned(),
                Yaml::str(format!(
                    "bash scripts/qualification/paired-sdk-run.sh \"{ARTIFACT_DIR_SHELL}\""
                )),
            ),
        ]),
    ]
}

fn build_digest_output_step() -> Yaml {
    let binary = format!("{ARTIFACT_DIR_SHELL}/{BINARY_NAME}");
    let manifest = format!("{ARTIFACT_DIR_SHELL}/{MANIFEST_NAME}");
    let run = format!(
        "set -eu\noutput=$(bash scripts/qualification/paired-sdk-build.sh \"{ARTIFACT_DIR_SHELL}\")\ncase \"$output\" in binary_sha256=*) digest=${{output#binary_sha256=}} ;; *) exit 1 ;; esac\ncase \"$digest\" in *[!0123456789abcdef]*|'') exit 1 ;; esac\ntest \"${{#digest}}\" -eq 64\nprintf 'binary_sha256=%s\\n' \"$digest\" >> \"$GITHUB_OUTPUT\"\ntest -f \"{binary}\" && test -s \"{binary}\"\ntest -f \"{manifest}\" && test -s \"{manifest}\""
    );
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Build and record the fixture digest"),
        ),
        ("id".to_owned(), Yaml::str("build")),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

fn upload_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Upload paired SDK fixture")),
        ("id".to_owned(), Yaml::str("upload")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::UPLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(ARTIFACT_NAME)),
                (
                    "path".to_owned(),
                    Yaml::str(format!(
                        "{ARTIFACT_DIR_ACTION}/{BINARY_NAME}\n{ARTIFACT_DIR_ACTION}/{MANIFEST_NAME}"
                    )),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("retention-days".to_owned(), Yaml::Int(1)),
            ]),
        ),
    ])
}

fn download_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Download this run's fixture")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::DOWNLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "artifact-ids".to_owned(),
                    Yaml::str("${{ needs.paired-sdk-build.outputs.artifact_id }}"),
                ),
                ("path".to_owned(), Yaml::str(ARTIFACT_DIR_ACTION)),
            ]),
        ),
    ])
}

pub(super) fn builder_outputs() -> Yaml {
    Yaml::Map(vec![
        (
            "artifact_id".to_owned(),
            Yaml::str("${{ steps.upload.outputs.artifact-id }}"),
        ),
        (
            "binary_sha256".to_owned(),
            Yaml::str("${{ steps.build.outputs.binary_sha256 }}"),
        ),
    ])
}

pub(super) fn builder_permissions() -> Yaml {
    permissions(&[("actions", "write"), ("contents", "read")])
}

pub(super) fn consumer_permissions() -> Yaml {
    permissions(&[("actions", "read"), ("contents", "read")])
}

fn permissions(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}
