// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{
    GitHubEvidenceFetchSpecV1, MiseTaskSuite, RELEASE_MANIFEST_FILENAME, ReleaseManifest,
    ReleaseTarget, Step, StepKind, check_release_artifact,
};

use super::MiseTaskSuitePolicy;
use crate::closure::{HelperProvenance, provision_acquire_step};
use crate::{MiseTaskSuitesRenderSpec, RenderError, Yaml, document_steps, steps};

pub(super) fn validate_policy(
    policy: &MiseTaskSuitePolicy,
    spec: &MiseTaskSuitesRenderSpec,
) -> Result<(), RenderError> {
    let has_fetch_spec = policy.evidence_fetch_spec.is_some();
    let has_helper = policy.evidence_helper.is_some();
    let has_snapshot = policy.suite.evidence_snapshot.is_some();
    if has_fetch_spec != has_helper
        || has_snapshot != (has_fetch_spec && has_helper)
        || ((has_fetch_spec || has_helper || has_snapshot)
            && policy.suite.events.as_slice()
                != [velnor_actions_contract::MiseTaskSuiteEvent::Schedule])
    {
        return Err(RenderError::InvalidWorkflow(
            "github_evidence_policy_shape".to_owned(),
        ));
    }
    if let Some(helper) = &policy.evidence_helper
        && (Some(helper.target())
            != velnor_actions_contract::ReleaseTarget::for_runner_label(&policy.runner_label)
            || helper.version() != env!("CARGO_PKG_VERSION")
            || helper.version() != spec.generator_version)
    {
        return Err(RenderError::InvalidWorkflow(
            "github_evidence_helper_binding".to_owned(),
        ));
    }
    if let Some(fetch_spec) = &policy.evidence_fetch_spec {
        fetch_spec.validate().map_err(|problem| {
            RenderError::InvalidWorkflow(format!("github_evidence_spec:{problem}"))
        })?;
        let suite_inventory = spec
            .suites
            .iter()
            .map(|candidate| candidate.suite.clone())
            .collect::<Vec<_>>();
        let resolved = policy
            .suite
            .evidence_snapshot
            .as_ref()
            .ok_or_else(|| {
                RenderError::InvalidWorkflow("github_evidence_input_missing".to_owned())
            })?
            .resolve_spec(&suite_inventory, &policy.suite.id, &spec.default_branch)
            .map_err(RenderError::Contract)?;
        if &resolved != fetch_spec
            || fetch_spec.collector_suite_id != policy.suite.id
            || fetch_spec.collector_workflow_path
                != format!(".github/workflows/{}.yml", policy.suite.workflow)
        {
            return Err(RenderError::InvalidWorkflow(
                "github_evidence_spec_configuration_mismatch".to_owned(),
            ));
        }
    }
    Ok(())
}

pub(super) fn evidence_collector_condition(suite: &MiseTaskSuite, default_branch: &str) -> String {
    suite
        .schedule
        .iter()
        .map(|cron| {
            format!(
                "github.event_name == 'schedule' && github.ref == 'refs/heads/{default_branch}' && github.event.schedule == '{cron}'"
            )
        })
        .collect::<Vec<_>>()
        .join(" || ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceHelperProvenance {
    version: String,
    target: ReleaseTarget,
    url: String,
    sha256: String,
    commit: String,
}

impl EvidenceHelperProvenance {
    pub fn from_release_manifest(
        manifest: &ReleaseManifest,
        runner_label: &str,
        expected_version: &str,
    ) -> Result<Self, RenderError> {
        manifest
            .validate(RELEASE_MANIFEST_FILENAME)
            .map_err(RenderError::Contract)?;
        let target = ReleaseTarget::for_runner_label(runner_label).ok_or_else(|| {
            RenderError::InvalidWorkflow("github_evidence_helper_target".to_owned())
        })?;
        if manifest.version != expected_version || expected_version != env!("CARGO_PKG_VERSION") {
            return Err(RenderError::InvalidWorkflow(
                "github_evidence_helper_version".to_owned(),
            ));
        }
        let record = manifest.record_for_target(target.triple()).ok_or_else(|| {
            RenderError::InvalidWorkflow("github_evidence_helper_target_missing".to_owned())
        })?;
        check_release_artifact(
            &record.artifact,
            &manifest.version,
            &manifest.commit,
            target.triple(),
            RELEASE_MANIFEST_FILENAME,
            "targets.artifact",
        )
        .map_err(RenderError::Contract)?;
        Ok(Self {
            version: manifest.version.clone(),
            target,
            url: record.artifact.clone(),
            sha256: record.sha256.clone(),
            commit: manifest.commit.clone(),
        })
    }

    pub(super) const fn target(&self) -> ReleaseTarget {
        self.target
    }

    pub(super) fn version(&self) -> &str {
        &self.version
    }

    pub(super) fn acquire_step(&self) -> Result<Step, RenderError> {
        let provenance = HelperProvenance::ReleaseAsset {
            url: self.url.clone(),
            sha256: self.sha256.clone(),
            commit: self.commit.clone(),
        };
        provision_acquire_step(
            &provenance,
            evidence_acquire_argv(&self.version, self.target),
        )
    }
}

fn evidence_acquire_argv(version: &str, target: ReleaseTarget) -> Vec<String> {
    let staged = format!("{}{}", steps::STAGED_BINARY_PREFIX, version);
    let digest = match target {
        ReleaseTarget::LinuxX86_64 => "sha256sum -c -",
        ReleaseTarget::MacosArm64 | ReleaseTarget::MacosX86_64 => "shasum -a 256 -c -",
    };
    let script = format!(
        "d=\"{staged}\"&&mkdir -p \"${{d%/*}}\"&&s=\"/opt/velnor/seed/generator/${{d##*/}}\"&&p=\"$VELNOR_ASSET_SHA256  \"&&if [ -f \"$s\" ]&&echo \"$p$s\"|{digest};then cp \"$s\" \"$d\";else curl -fsSL --connect-timeout 15 --max-time 120 --retry 5 --retry-all-errors --retry-max-time 180 --proto '=https' --tlsv1.2 \"$VELNOR_ASSET_URL\" -o \"$d\"&&echo \"$p$d\"|{digest};fi&&chmod +x \"$d\""
    );
    vec!["sh".to_owned(), "-c".to_owned(), script]
}

pub(super) fn job_step_to_yaml(
    policy: &MiseTaskSuitePolicy,
    step: &Step,
) -> Result<Yaml, RenderError> {
    if let StepKind::Internal {
        operation,
        env: step_env,
    } = &step.kind
    {
        let (role, token) = match operation.as_str() {
            velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION => {
                (velnor_actions_contract::StepRole::GitHubEvidenceFetch, true)
            }
            velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION => (
                velnor_actions_contract::StepRole::GitHubEvidenceImport,
                false,
            ),
            _ => {
                return Err(RenderError::InvalidWorkflow(
                    "github_evidence_internal_operation".to_owned(),
                ));
            }
        };
        let expected_len = if token { 2 } else { 1 };
        if step.role != Some(role)
            || step.id.is_some()
            || step.parallel
            || step.condition.is_some()
            || step_env.len() != expected_len
            || step_env
                .get(velnor_actions_contract::GITHUB_EVIDENCE_SPEC_ENV)
                .is_none_or(String::is_empty)
            || (token
                && step_env
                    .get(velnor_actions_contract::GITHUB_EVIDENCE_TOKEN_ENV)
                    .map(String::as_str)
                    != Some("${{ github.token }}"))
            || (!token && step_env.contains_key(velnor_actions_contract::GITHUB_EVIDENCE_TOKEN_ENV))
        {
            return Err(RenderError::InvalidWorkflow(
                "github_evidence_internal_shape".to_owned(),
            ));
        }
        let binary = format!(
            "{}{}",
            steps::STAGED_BINARY_PREFIX,
            policy_generator_version(policy)
        );
        let mut env = vec![(
            steps::INTERNAL_OP_ENV.to_owned(),
            Yaml::str(operation.clone()),
        )];
        env.extend(
            step_env
                .iter()
                .map(|(key, value)| (key.clone(), Yaml::str(value.clone()))),
        );
        return Ok(Yaml::Map(vec![
            ("name".to_owned(), Yaml::str(step.name.clone())),
            ("env".to_owned(), Yaml::Map(env)),
            (
                "run".to_owned(),
                Yaml::str(crate::commands::quote_run_arg(&binary)?),
            ),
        ]));
    }
    document_steps::non_internal_step_to_yaml(
        &format!("mise-task-{}", policy.suite.id),
        step,
        &BTreeMap::new(),
        &policy.runner_label,
    )
}

fn policy_generator_version(_policy: &MiseTaskSuitePolicy) -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub(super) fn github_evidence_internal_step(
    operation: &str,
    spec: &GitHubEvidenceFetchSpecV1,
    token: Option<&str>,
) -> Result<Step, RenderError> {
    let spec_json =
        velnor_actions_contract::canonical_json_str(spec).map_err(RenderError::Contract)?;
    let mut env = BTreeMap::from([(
        velnor_actions_contract::GITHUB_EVIDENCE_SPEC_ENV.to_owned(),
        spec_json,
    )]);
    if let Some(token) = token {
        env.insert(
            velnor_actions_contract::GITHUB_EVIDENCE_TOKEN_ENV.to_owned(),
            token.to_owned(),
        );
    }
    Ok(Step {
        name: if token.is_some() {
            "Fetch GitHub evidence snapshot".to_owned()
        } else {
            "Import GitHub evidence snapshot".to_owned()
        },
        id: None,
        role: Some(if token.is_some() {
            velnor_actions_contract::StepRole::GitHubEvidenceFetch
        } else {
            velnor_actions_contract::StepRole::GitHubEvidenceImport
        }),
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: operation.to_owned(),
            env,
        },
    })
}

pub(super) fn evidence_fetch_job_id(suite_id: &str) -> String {
    format!("github-evidence-fetch-{suite_id}")
}

pub(super) fn evidence_bundle_name(suite_id: &str) -> String {
    format!(
        "velnor-github-evidence-{}-{suite_id}-${{{{ github.run_id }}}}-attempt-${{{{ github.run_attempt }}}}",
        suite_id.len()
    )
}

pub(super) fn evidence_fetch_job_yaml(
    policy: &MiseTaskSuitePolicy,
    spec: &MiseTaskSuitesRenderSpec,
) -> Result<Yaml, RenderError> {
    let fetch_steps = fixed_fetch_steps(policy, spec)?;
    validate_fetch_step_inventory(policy, spec, &fetch_steps)?;
    let steps_yaml = fetch_steps
        .iter()
        .map(|step| job_step_to_yaml(policy, step))
        .collect::<Result<Vec<_>, _>>()?;
    let schedule = policy
        .suite
        .schedule
        .iter()
        .map(|cron| format!("github.event.schedule == '{cron}'"))
        .collect::<Vec<_>>()
        .join(" || ");
    let condition = format!(
        "github.event_name == 'schedule' && github.ref == 'refs/heads/{}' && ({schedule})",
        spec.default_branch
    );
    steps::scan_for_private_subcommands(&condition)?;
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(format!("Fetch GitHub evidence ({})", policy.suite.id)),
        ),
        ("runs-on".to_owned(), Yaml::str(policy.runner_label.clone())),
        ("timeout-minutes".to_owned(), Yaml::Int(30)),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![
                ("actions".to_owned(), Yaml::str("read")),
                ("contents".to_owned(), Yaml::str("read")),
            ]),
        ),
        ("if".to_owned(), Yaml::str(condition)),
        ("steps".to_owned(), Yaml::Seq(steps_yaml)),
    ]))
}

pub(super) fn fixed_fetch_steps(
    policy: &MiseTaskSuitePolicy,
    render_spec: &MiseTaskSuitesRenderSpec,
) -> Result<Vec<Step>, RenderError> {
    let helper = policy
        .evidence_helper
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("github_evidence_helper_missing".to_owned()))?;
    let fetch_spec = policy.evidence_fetch_spec.as_ref().ok_or_else(|| {
        RenderError::InvalidWorkflow("github_evidence_fetch_spec_missing".to_owned())
    })?;
    if Some(helper.target()) != ReleaseTarget::for_runner_label(&policy.runner_label)
        || helper.version() != render_spec.generator_version
        || helper.version() != env!("CARGO_PKG_VERSION")
    {
        return Err(RenderError::InvalidWorkflow(
            "github_evidence_helper_binding".to_owned(),
        ));
    }
    let upload = crate::steps::action_step(
        "Upload GitHub evidence snapshot",
        steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), evidence_bundle_name(&policy.suite.id)),
            (
                "path".to_owned(),
                "${{ runner.temp }}/velnor/github-evidence-bundle".to_owned(),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "30".to_owned()),
        ]),
    )?;
    Ok(vec![
        helper.acquire_step()?,
        github_evidence_internal_step(
            velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION,
            fetch_spec,
            Some("${{ github.token }}"),
        )?,
        upload,
    ])
}

pub(super) fn validate_fetch_step_inventory(
    policy: &MiseTaskSuitePolicy,
    render_spec: &MiseTaskSuitesRenderSpec,
    candidate: &[Step],
) -> Result<(), RenderError> {
    if fixed_fetch_steps(policy, render_spec)? != candidate {
        return Err(RenderError::InvalidWorkflow(
            "github_evidence_fetch_step_inventory".to_owned(),
        ));
    }
    Ok(())
}
