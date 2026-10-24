//! Closed preview workflow steps; no source code executes in privileged jobs.

use super::{ConsumerReleaseSpec, MiseSetup, PackageReleasePolicy, RenderError, Yaml};
use crate::steps::CHECKOUT_USES;

// runner.temp expressions are supported at step env scope, never job env scope.
pub(in crate::schema2) fn with_producer_homes(
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

pub(in crate::schema2) fn permissions(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

pub(in crate::schema2) fn needs(values: &[&str]) -> Yaml {
    Yaml::Seq(values.iter().map(|value| Yaml::str(*value)).collect())
}

pub(in crate::schema2) fn outputs(values: &[&str]) -> Yaml {
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

pub(in crate::schema2) fn setup(setup: &MiseSetup) -> Yaml {
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

pub(in crate::schema2) fn script(name: &str, id: &str, run: &str, env: &[(&str, &str)]) -> Yaml {
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("shell".to_owned(), Yaml::str("bash")),
    ];
    if !env.is_empty() {
        fields.push(("env".to_owned(), permissions(env)));
    }
    fields.push(("run".to_owned(), Yaml::str(run)));
    Yaml::Map(fields)
}

pub(in crate::schema2) fn checkout(spec: &ConsumerReleaseSpec, source_ref: &str) -> Yaml {
    let mut inputs = vec![
        ("ref".to_owned(), Yaml::str(source_ref)),
        ("fetch-depth".to_owned(), Yaml::Int(0)),
        ("persist-credentials".to_owned(), Yaml::Bool(false)),
    ];
    if spec.checkout_lfs {
        inputs.push(("lfs".to_owned(), Yaml::str("true")));
    }
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Check out exact source with complete history"),
        ),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        ("with".to_owned(), Yaml::Map(inputs)),
    ])
}

pub(in crate::schema2) fn checkout_with_lfs(
    spec: &ConsumerReleaseSpec,
    source_ref: &str,
    lfs: &str,
) -> Yaml {
    let mut step = checkout(spec, source_ref);
    if let Yaml::Map(fields) = &mut step
        && let Some((_, Yaml::Map(inputs))) = fields.iter_mut().find(|(key, _)| key == "with")
    {
        if let Some((_, value)) = inputs.iter_mut().find(|(key, _)| key == "lfs") {
            *value = Yaml::str(lfs);
        } else {
            inputs.push(("lfs".to_owned(), Yaml::str(lfs)));
        }
    }
    step
}

#[path = "schema2_consumer_release_env.rs"]
mod native_env;

pub(in crate::schema2) fn operation(
    spec: &ConsumerReleaseSpec,
    op: &str,
    authenticated: bool,
    context: super::TapInvocationContext,
) -> Result<Yaml, RenderError> {
    let environment = native_env::environment(spec, op, authenticated, context)?;
    // JSON is an environment value, never interpolated into the command source.
    let materialize = if matches!(
        op,
        "sign-package-release-v1"
            | "prepare-homebrew-tap-v1"
            | "execute-release-producer-v1"
            | "finalize-preview-source-v1"
            | "acquire-preview-source-v1"
            | "acquire-release-leaves-v1"
            | "admit-release-aggregate-v1"
            | "publish-package-release-v1"
    ) || (spec.policy.channel
        == velnor_actions_contract::package_release::PackageReleaseChannel::Stable
        && matches!(
            op,
            "write-stable-tag-origin-v1"
                | "admit-release-aggregate-v1"
                | "publish-package-release-v1"
        )) {
        // The preceding native admission already created and validated this file.
        ""
    } else {
        "(set -C; printf '%s' \"$VELNOR_RELEASE_POLICY\" > \"$VELNOR_REQUEST_FILE\")\n"
    };
    let run = format!(
        "set -euo pipefail\numask 077\nmkdir -p \"$RUNNER_TEMP/velnor\"\n{materialize}\"$RUNNER_TEMP/velnor/bin/velnor-actions-{}\"\n",
        spec.generator_version
    );
    let run = source_operation_script(op, run);
    let mut step = script("Execute native release operation", "release", &run, &[]);
    if let Yaml::Map(fields) = &mut step {
        let Some(run_index) = fields.iter().position(|(key, _)| key == "run") else {
            return Err(RenderError::InvalidWorkflow(
                "operation_run_step_missing".to_owned(),
            ));
        };
        fields.insert(run_index, ("env".to_owned(), environment));
    }
    Ok(step)
}

fn source_operation_script(op: &str, run: String) -> String {
    if matches!(
        op,
        "execute-release-producer-v1" | "prepare-homebrew-tap-v1"
    ) {
        // Source preparation may have written runner environment files. Remove
        // credentials before the native source executor itself is spawned.
        crate::toolchain_env::with_credential_unset_script(&format!(
            "unset VELNOR_RELEASE_STATUS_TOKEN; {run}"
        ))
    } else {
        run
    }
}

#[path = "schema2_consumer_release_evidence.rs"]
mod evidence;
pub(in crate::schema2) use evidence::{Artifact, attest, transfer};

pub(in crate::schema2) fn history_checkout(spec: &ConsumerReleaseSpec) -> Yaml {
    checkout(spec, "${{ github.sha }}")
}

pub(in crate::schema2) fn history_count(context: super::TapInvocationContext) -> Yaml {
    if matches!(
        context,
        super::TapInvocationContext::Stable
            | super::TapInvocationContext::StableAuthority
            | super::TapInvocationContext::StableReconcile
    ) {
        return script(
            "Bind the stable source version",
            "history",
            r#"set -euo pipefail
printf 'commit_count=0\n' >> "$GITHUB_OUTPUT"
"#,
            &[],
        );
    }

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
            match context {
                super::TapInvocationContext::Preview
                | super::TapInvocationContext::Stable
                | super::TapInvocationContext::StableAuthority => {
                    "${{ steps.source.outputs.source_sha }}"
                }
                super::TapInvocationContext::Reconcile
                | super::TapInvocationContext::StableReconcile => "${{ github.sha }}",
            },
        )],
    )
}

pub(in crate::schema2) fn recovery_inputs() -> Yaml {
    let inputs = [
        (
            "original_run_id",
            "Original preview production workflow run ID",
        ),
        (
            "original_run_attempt",
            "Original preview production workflow run attempt",
        ),
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
