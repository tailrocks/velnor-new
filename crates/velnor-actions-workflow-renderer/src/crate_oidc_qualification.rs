// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::{
    RenderError, ambient_shell_step, release_generator_bootstrap,
    release_spec::{validate_environment, validate_repository},
    yaml::Yaml,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcQualificationSpec {
    pub repository: String,
    pub default_branch: String,
    pub environment: String,
    pub required_workflow_path: String,
    pub trusted_reviewer_id: u64,
    pub generator_version: String,
    pub generator_source_sha: String,
    pub generator_manifest_sha256: String,
}

impl OidcQualificationSpec {
    pub fn validate_intent(
        &self,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
    ) -> Result<(), RenderError> {
        self.validate()?;
        intent.validate().map_err(RenderError::Contract)?;
        if !intent.oidc_qualification
            || self.repository != intent.repository
            || self.default_branch != intent.default_branch
            || self.environment != intent.environment
            || self.required_workflow_path != intent.required_workflow_path
            || self.trusted_reviewer_id != intent.trusted_reviewer_id
            || self.generator_version != intent.generator_version
            || self.generator_source_sha != intent.generator_source_sha
            || self.generator_manifest_sha256 != intent.generator_manifest_sha256
            || intent.authentication
                != velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing
            || intent.new_crates.is_some()
        {
            return Err(RenderError::InvalidWorkflow(
                "oidc_qualification_intent_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), RenderError> {
        validate_repository(&self.repository)?;
        validate_environment(&self.environment)?;
        let workflow = self
            .required_workflow_path
            .strip_prefix(".github/workflows/");
        let valid_workflow = workflow.is_some_and(|name| {
            !name.is_empty()
                && !name.contains('/')
                && name
                    .rsplit_once('.')
                    .is_some_and(|(_, ext)| matches!(ext, "yml" | "yaml"))
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        });
        if !velnor_actions_contract::is_valid_branch_name(&self.default_branch)
            || self.trusted_reviewer_id == 0
            || !valid_workflow
        {
            return Err(RenderError::InvalidWorkflow(
                "oidc_qualification_binding".to_owned(),
            ));
        }
        release_generator_bootstrap::acquire_step(
            &self.generator_version,
            crate::release_tree::RELEASE_HOSTED_RUNNER,
            &self.generator_source_sha,
            &self.generator_manifest_sha256,
        )?;
        Ok(())
    }

    pub fn job(&self) -> Result<Yaml, RenderError> {
        self.validate()?;
        let mut acquire =
            crate::steps_plain::plain_step_to_yaml(&release_generator_bootstrap::acquire_step(
                &self.generator_version,
                crate::release_tree::RELEASE_HOSTED_RUNNER,
                &self.generator_source_sha,
                &self.generator_manifest_sha256,
            )?)?;
        secure_acquire(&mut acquire)?;
        let admission = self.admission_step()?;
        let probe = probe_step();
        Ok(Yaml::Map(vec![
            ("name".into(), Yaml::str("OIDC qualification")),
            (
                "runs-on".into(),
                Yaml::str(crate::release_tree::RELEASE_HOSTED_RUNNER),
            ),
            ("timeout-minutes".into(), Yaml::Int(10)),
            ("environment".into(), Yaml::str(self.environment.clone())),
            (
                "permissions".into(),
                Yaml::Map(vec![
                    ("actions".into(), Yaml::str("read")),
                    ("checks".into(), Yaml::str("read")),
                    ("contents".into(), Yaml::str("read")),
                    ("pull-requests".into(), Yaml::str("read")),
                    ("id-token".into(), Yaml::str("write")),
                ]),
            ),
            (
                "if".into(),
                Yaml::str(format!(
                    "github.repository == '{}' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/{}' && github.event.inputs.mode == 'oidc-qualification'",
                    self.repository, self.default_branch
                )),
            ),
            ("steps".into(), Yaml::Seq(vec![acquire, admission, probe])),
        ]))
    }

    fn admission_step(&self) -> Result<Yaml, RenderError> {
        let mut env = BTreeMap::from([
            (
                "GIT_TOKEN".to_owned(),
                crate::release_gates::GIT_TOKEN_REF.to_owned(),
            ),
            (
                "VELNOR_QUALIFICATION_DEFAULT_BRANCH".to_owned(),
                self.default_branch.clone(),
            ),
            (
                "VELNOR_QUALIFICATION_ENVIRONMENT".to_owned(),
                self.environment.clone(),
            ),
            (
                "VELNOR_QUALIFICATION_REQUIRED_WORKFLOW".to_owned(),
                self.required_workflow_path.clone(),
            ),
            (
                "VELNOR_QUALIFICATION_REVIEWER_ID".to_owned(),
                self.trusted_reviewer_id.to_string(),
            ),
        ]);
        let step = ambient_shell_step(
            "Verify OIDC qualification admission",
            vec![
                format!(
                    "$RUNNER_TEMP/velnor/bin/velnor-actions-{}",
                    self.generator_version
                ),
                "crate-release".into(),
                "oidc-qualification-admit".into(),
            ],
            std::mem::take(&mut env),
        )?;
        let mut rendered = crate::steps_plain::plain_step_to_yaml(&step)?;
        let Yaml::Map(fields) = &mut rendered else {
            return Err(RenderError::InvalidWorkflow(
                "oidc_qualification_step".to_owned(),
            ));
        };
        let Some((_, Yaml::Map(values))) = fields.iter_mut().find(|(key, _)| key == "env") else {
            return Err(RenderError::InvalidWorkflow(
                "oidc_qualification_step".to_owned(),
            ));
        };
        values.push((
            "VELNOR_QUALIFICATION_MODE".to_owned(),
            Yaml::str("${{ github.event.inputs.mode }}"),
        ));
        secure_acquire(&mut rendered)?;
        Ok(rendered)
    }
}

fn secure_acquire(step: &mut Yaml) -> Result<(), RenderError> {
    let Yaml::Map(fields) = step else {
        return Err(RenderError::InvalidWorkflow(
            "oidc_qualification_step".to_owned(),
        ));
    };
    let Some((_, Yaml::Str(run))) = fields.iter_mut().find(|(key, _)| key == "run") else {
        return Err(RenderError::InvalidWorkflow(
            "oidc_qualification_step".to_owned(),
        ));
    };
    *run = format!(
        "unset BASH_ENV ENV SSLKEYLOGFILE PYTHONHOME PYTHONPATH ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL; {run}"
    );
    fields.push((
        "shell".into(),
        Yaml::str("/bin/bash --noprofile --norc -p -e -o pipefail {0}"),
    ));
    Ok(())
}

fn probe_step() -> Yaml {
    let launcher = include_str!("crate_oidc_probe_launcher.sh");
    let probe = include_str!("crate_oidc_probe.py");
    let command = launcher.replace(
        "exec /usr/bin/python3 -I -S -",
        &format!(
            "exec /usr/bin/python3 -I -S - <<'VELNOR_OIDC_PROBE_PY'\n{probe}\nVELNOR_OIDC_PROBE_PY"
        ),
    );
    Yaml::Map(vec![
        (
            "name".into(),
            Yaml::str("Exchange and revoke qualification credential"),
        ),
        (
            "shell".into(),
            Yaml::str("/bin/bash --noprofile --norc -p -e -o pipefail {0}"),
        ),
        ("run".into(), Yaml::str(command)),
    ])
}

impl OidcQualificationSpec {
    pub fn validate_jobs(
        &self,
        job: &Yaml,
        invalid: &Yaml,
        release_dispatch: Option<&crate::release_spec::ReleasePlan>,
    ) -> Result<(), RenderError> {
        if *job != self.job()? || *invalid != self.invalid_dispatch_job(release_dispatch)? {
            return Err(RenderError::InvalidWorkflow(
                "oidc_qualification_job_structure".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn invalid_dispatch_job(
        &self,
        release_dispatch: Option<&crate::release_spec::ReleasePlan>,
    ) -> Result<Yaml, RenderError> {
        self.validate()?;
        let allowed_mode = if let Some(plan) = release_dispatch {
            format!(
                "({} || github.event.inputs.mode == 'oidc-qualification')",
                crate::release_spec::release_dispatch_condition(&self.default_branch, plan)
            )
        } else {
            "github.event.inputs.mode == 'oidc-qualification'".to_owned()
        };
        let condition = format!(
            "github.event_name == 'workflow_dispatch' && !(github.repository == '{}' && github.ref == 'refs/heads/{}' && {allowed_mode})",
            self.repository, self.default_branch
        );
        Ok(Yaml::Map(vec![
            ("name".into(), Yaml::str("Reject invalid release dispatch")),
            (
                "runs-on".into(),
                Yaml::str(crate::release_tree::RELEASE_HOSTED_RUNNER),
            ),
            ("permissions".into(), Yaml::Map(Vec::new())),
            ("timeout-minutes".into(), Yaml::Int(5)),
            ("if".into(), Yaml::str(condition)),
            (
                "steps".into(),
                Yaml::Seq(vec![Yaml::Map(vec![
                    ("name".into(), Yaml::str("Reject unsupported dispatch")),
                    ("run".into(), Yaml::str("exit 1")),
                ])]),
            ),
        ]))
    }
}
