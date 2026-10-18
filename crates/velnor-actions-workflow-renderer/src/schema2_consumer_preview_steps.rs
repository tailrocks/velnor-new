//! Closed preview workflow steps; no source code executes in privileged jobs.

use super::{ConsumerPreviewSpec, MiseSetup, PackageReleasePlan, RenderError, Yaml};
use crate::steps::{
    ATTEST_BUILD_PROVENANCE_USES, CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES,
};

// runner.temp expressions are supported at step env scope, never job env scope.
pub(super) fn with_producer_homes(
    step: &mut Yaml,
    homes: &std::collections::BTreeMap<String, String>,
) -> Result<(), RenderError> {
    let Yaml::Map(fields) = step else {
        return Err(RenderError::InvalidWorkflow(
            "preview_step_not_map".to_owned(),
        ));
    };
    if !fields.iter().any(|(key, _)| key == "env") {
        fields.push(("env".to_owned(), Yaml::Map(Vec::new())));
    }
    let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(key, _)| key == "env") else {
        return Err(RenderError::InvalidWorkflow(
            "preview_step_env_not_map".to_owned(),
        ));
    };
    for (key, value) in homes {
        let expected = Yaml::str(value);
        if let Some((_, existing)) = env.iter().find(|(name, _)| name == key) {
            if existing != &expected {
                return Err(RenderError::InvalidWorkflow(
                    "preview_step_home_conflict".to_owned(),
                ));
            }
        } else {
            env.push((key.clone(), expected));
        }
    }
    Ok(())
}

pub(super) fn permissions(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

pub(super) fn needs(values: &[&str]) -> Yaml {
    Yaml::Seq(values.iter().map(|value| Yaml::str(*value)).collect())
}

pub(super) fn outputs(values: &[&str]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|key| {
                (
                    (*key).to_owned(),
                    Yaml::str(format!("${{{{ steps.source.outputs.{key} }}}}")),
                )
            })
            .collect(),
    )
}

pub(super) fn setup(setup: &MiseSetup) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Set up Velnor-owned Mise")),
        ("uses".to_owned(), Yaml::str(&setup.uses)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("version".to_owned(), Yaml::str(&setup.version)),
                ("sha256".to_owned(), Yaml::str(&setup.sha256)),
                ("install".to_owned(), Yaml::Bool(false)),
                ("env".to_owned(), Yaml::Bool(false)),
                ("cache".to_owned(), Yaml::Bool(false)),
                ("cache_save".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

pub(super) fn script(name: &str, id: &str, run: &str, env: &[(&str, &str)]) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("env".to_owned(), permissions(env)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

pub(super) fn checkout(source_ref: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Check out exact source with complete history"),
        ),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("ref".to_owned(), Yaml::str(source_ref)),
                ("fetch-depth".to_owned(), Yaml::Int(0)),
                ("persist-credentials".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ])
}

pub(super) fn operation(
    spec: &ConsumerPreviewSpec,
    op: &str,
    authenticated: bool,
) -> Result<Yaml, RenderError> {
    let plan = String::from_utf8(
        spec.plan
            .canonical_bytes()
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?,
    )
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let mut env = vec![
        ("VELNOR_RELEASE_PLAN", plan.as_str()),
        ("VELNOR_INTERNAL_OP", op),
        (
            "VELNOR_REQUEST_FILE",
            "${{ runner.temp }}/velnor/package-release-plan.json",
        ),
        (
            "VELNOR_RELEASE_COMMIT_COUNT",
            "${{ needs.preview-eligibility.outputs.commit_count }}",
        ),
        (
            "VELNOR_RELEASE_CI_RUN_ID",
            "${{ needs.preview-eligibility.outputs.ci_run_id }}",
        ),
        (
            "VELNOR_RELEASE_CI_RUN_ATTEMPT",
            "${{ needs.preview-eligibility.outputs.ci_attempt }}",
        ),
    ];
    if matches!(
        op,
        "recover-package-release-v1"
            | "prepare-homebrew-tap-v1"
            | "write-homebrew-tap-pr-v1"
            | "merge-homebrew-tap-pr-v1"
    ) {
        env.extend([
            (
                "VELNOR_RELEASE_ORIGINAL_RUN_ID",
                "${{ inputs.original_run_id }}",
            ),
            (
                "VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT",
                "${{ inputs.original_run_attempt }}",
            ),
        ]);
    }
    if authenticated {
        env.push(("GH_TOKEN", "${{ github.token }}"));
    }
    if matches!(
        op,
        "publish-package-release-v1" | "recover-package-release-v1"
    ) {
        env.push((
            "VELNOR_RELEASE_STATUS_TOKEN",
            "${{ secrets.PREVIEW_IMMUTABILITY_READ_TOKEN }}",
        ));
    }
    // JSON is an environment value, never interpolated into the command source.
    let run = format!(
        "set -euo pipefail\numask 077\nmkdir -p \"$RUNNER_TEMP/velnor\"\n(set -C; printf '%s' \"$VELNOR_RELEASE_PLAN\" > \"$VELNOR_REQUEST_FILE\")\n\"$RUNNER_TEMP/velnor/bin/velnor-actions-{}\"\n",
        spec.generator_version
    );
    Ok(script(
        "Execute native release operation",
        "release",
        &run,
        &env,
    ))
}

#[derive(Clone, Copy)]
pub(super) enum Artifact {
    Producer,
    Signed,
    TapCandidate,
    TapPr,
}

pub(super) fn transfer(download: bool, artifact: Artifact) -> Yaml {
    let (suffix, directory) = match (download, artifact) {
        (true, Artifact::Producer) => ("producer", "preview-input"),
        (false, Artifact::Producer) => ("producer", "preview-output"),
        (true, Artifact::Signed) => ("signed", "preview-publish-input"),
        (false, Artifact::Signed) => ("signed", "preview-signed"),
        (_, Artifact::TapCandidate) => ("tap-candidate", "tap-candidate"),
        (true, Artifact::TapPr) => ("tap-pr", ""),
        (false, Artifact::TapPr) => ("tap-pr", "tap-pr.json"),
    };
    let mut with = vec![
        (
            "name".to_owned(),
            Yaml::str(format!(
                "velnor-preview-{suffix}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}"
            )),
        ),
        (
            "path".to_owned(),
            Yaml::str(format!("${{{{ runner.temp }}}}/velnor/{directory}")),
        ),
    ];
    if !download {
        with.extend([
            ("if-no-files-found".to_owned(), Yaml::str("error")),
            ("retention-days".to_owned(), Yaml::Int(30)),
            ("overwrite".to_owned(), Yaml::Bool(false)),
            ("include-hidden-files".to_owned(), Yaml::Bool(false)),
        ]);
    }
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(if download {
                "Download exact release evidence"
            } else {
                "Retain original release evidence"
            }),
        ),
        (
            "uses".to_owned(),
            Yaml::str(if download {
                DOWNLOAD_ARTIFACT_USES
            } else {
                UPLOAD_ARTIFACT_USES
            }),
        ),
        ("with".to_owned(), Yaml::Map(with)),
    ])
}

pub(super) fn attest(plan: &PackageReleasePlan) -> Yaml {
    let prefix = "${{ runner.temp }}/velnor/preview-signed";
    let mut names: Vec<String> = plan
        .products
        .iter()
        .flat_map(|product| {
            product
                .targets
                .iter()
                .map(move |target| format!("{}-{target}.tar.gz", product.id))
        })
        .collect();
    names.extend(
        plan.companions
            .iter()
            .map(|companion| companion.asset_name.clone()),
    );
    names.push("release.json".to_owned());
    names.sort();
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Attest exact native release subjects"),
        ),
        ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("create-storage-record".to_owned(), Yaml::Bool(false)),
                (
                    "subject-path".to_owned(),
                    Yaml::str(
                        names
                            .iter()
                            .map(|name| format!("{prefix}/{name}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                ),
            ]),
        ),
    ])
}

pub(super) fn history_checkout() -> Yaml {
    checkout("${{ github.sha }}")
}

pub(super) fn history_count() -> Yaml {
    script(
        "Count complete source history",
        "history",
        r#"set -euo pipefail
[[ "$(git rev-parse --is-shallow-repository)" == false ]]
[[ "$(git rev-parse HEAD)" == "$EXPECTED_SOURCE_SHA" ]]
count=$(git rev-list --count "$EXPECTED_SOURCE_SHA")
[[ "$count" =~ ^[1-9][0-9]*$ ]]
printf 'commit_count=%s\n' "$count" >> "$GITHUB_OUTPUT"
"#,
        &[(
            "EXPECTED_SOURCE_SHA",
            "${{ steps.source.outputs.source_sha }}",
        )],
    )
}

pub(super) fn recovery_inputs() -> Yaml {
    let inputs = [
        ("original_run_id", "Original push workflow run ID"),
        ("original_run_attempt", "Original signer run attempt"),
    ]
    .into_iter()
    .map(|(key, description)| {
        (
            key.to_owned(),
            Yaml::Map(vec![
                ("description".to_owned(), Yaml::str(description)),
                ("required".to_owned(), Yaml::Bool(true)),
                ("type".to_owned(), Yaml::str("string")),
            ]),
        )
    })
    .collect();
    Yaml::Map(vec![("inputs".to_owned(), Yaml::Map(inputs))])
}
