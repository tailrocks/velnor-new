//! Generation-only authority for exact compiled `DockerHub` login steps.
use crate::RenderError;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    HelperInvocation, SourceBoundOperation, StepId, StepKind, WorkflowIr,
};

/// Inputs qualified by the compiled OCI owner, never repository deserialization.
#[derive(Debug, Clone)]
pub struct DockerRegistryLoginBinding {
    /// Reviewed catalog login action including its exact SHA.
    pub approved_login_uses: String,
    /// `DockerHub` registry literal.
    pub registry: String,
    /// Qualified named username secret.
    pub username_secret: String,
    /// Qualified named password secret.
    pub password_secret: String,
    /// Exact source repository.
    pub repository: String,
    /// Exact protected source branch checked by full CI admission.
    pub default_branch: String,
    /// Required external CI workflow.
    pub ci_workflow: String,
    /// Mandatory local full CI admission dependency.
    pub full_ci_job: String,
    /// Compiled source invocation proving source and external CI.
    pub full_ci_admission: HelperInvocation,
}

/// Exact graph approval, with private fields and no serialization surface.
#[derive(Debug, Clone)]
pub struct ActionCredentialApproval {
    job_id: String,
    step_id: StepId,
    workflow: WorkflowIr,
}

impl ActionCredentialApproval {
    /// Freeze a qualified OCI graph and its exact `DockerHub` named-secret login.
    /// The compiled factory must supply reviewed pins and qualified source records.
    /// # Errors
    /// Rejects missing source proof, malformed secret bindings or changed login shape.
    pub fn docker_registry_login(
        job_id: &str,
        step_id: &StepId,
        binding: &DockerRegistryLoginBinding,
        workflow: &WorkflowIr,
    ) -> Result<Self, RenderError> {
        workflow.validate().map_err(RenderError::Contract)?;
        validate_source(binding, workflow)?;
        validate_login(job_id, step_id, binding, workflow)?;
        Ok(Self {
            job_id: job_id.into(),
            step_id: step_id.clone(),
            workflow: workflow.clone(),
        })
    }

    /// Admit only the complete unchanged graph and designated action step.
    #[must_use]
    pub fn admits(&self, job_id: &str, step_id: Option<&StepId>, workflow: &WorkflowIr) -> bool {
        self.job_id == job_id && step_id == Some(&self.step_id) && self.workflow == *workflow
    }

    pub(crate) fn matches_graph(&self, workflow: &WorkflowIr) -> bool {
        self.workflow == *workflow
    }
}

fn invalid() -> RenderError {
    RenderError::InvalidWorkflow("unqualified_docker_registry_login".into())
}

fn validate_source(
    binding: &DockerRegistryLoginBinding,
    workflow: &WorkflowIr,
) -> Result<(), RenderError> {
    let proof = workflow
        .jobs
        .get(&binding.full_ci_job)
        .ok_or_else(invalid)?;
    let expected = format!(
        "success() && github.repository == '{}' && startsWith(github.ref, 'refs/tags/v') && (github.event_name == 'push' || github.event_name == 'workflow_dispatch')",
        binding.repository
    );
    let args = [
        "verify",
        &binding.repository,
        &binding.ci_workflow,
        &binding.default_branch,
    ];
    if proof.condition.as_deref() != Some(expected.as_str())
        || binding.full_ci_admission.descriptor().operation() != SourceBoundOperation::OciDelivery
        || binding.full_ci_admission.args().iter().map(String::as_str).ne(args)
        || proof.steps.iter().filter(|step| step.condition.is_none() && matches!(&step.kind,
            StepKind::SourceBoundHelper { invocation, env } if invocation == &binding.full_ci_admission && source_environment(env))).count() != 1
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_login(
    job_id: &str,
    step_id: &StepId,
    binding: &DockerRegistryLoginBinding,
    workflow: &WorkflowIr,
) -> Result<(), RenderError> {
    let job = workflow.jobs.get(job_id).ok_or_else(invalid)?;
    let permissions = job.permissions.as_ref().unwrap_or(&workflow.permissions);
    if permissions != &velnor_actions_contract::Permissions::default()
        || job.native_pages_deploy.is_some()
        || job.source_producer.is_some()
        || job.tool_producer.is_some()
        || !successful_dependency(job.condition.as_deref(), &binding.full_ci_job)
    {
        return Err(invalid());
    }
    let step = job
        .steps
        .iter()
        .find(|step| step.id.as_ref() == Some(step_id))
        .ok_or_else(invalid)?;
    let Some(sha) = binding
        .approved_login_uses
        .strip_prefix("docker/login-action@")
    else {
        return Err(invalid());
    };
    if sha.len() != 40
        || !sha.bytes().all(|b| b.is_ascii_hexdigit())
        || binding.registry != "docker.io"
        || !secret_name(&binding.username_secret)
        || !secret_name(&binding.password_secret)
        || step.condition.is_some()
        || !job.needs.contains(&binding.full_ci_job)
        || job.native_publish.is_some()
    {
        return Err(invalid());
    }
    let with = BTreeMap::from([
        ("registry".into(), binding.registry.clone()),
        (
            "username".into(),
            format!("${{{{ secrets.{} }}}}", binding.username_secret),
        ),
        (
            "password".into(),
            format!("${{{{ secrets.{} }}}}", binding.password_secret),
        ),
    ]);
    let env = BTreeMap::from([(
        "DOCKER_CONFIG".into(),
        "${{ runner.temp }}/velnor/oci-docker".into(),
    )]);
    if !matches!(&step.kind, StepKind::Action { uses, with: actual, env: actual_env }
        if uses == &binding.approved_login_uses && actual == &with && actual_env == &env)
    {
        return Err(invalid());
    }
    Ok(())
}

fn secret_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with("GITHUB_")
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn source_environment(env: &BTreeMap<String, String>) -> bool {
    !env.contains_key("DOCKER_CONFIG")
        && [
            ("GH_TOKEN", "${{ github.token }}"),
            ("EVENT_NAME", "${{ github.event_name }}"),
            ("REF", "${{ github.ref }}"),
            ("SOURCE_SHA", "${{ github.sha }}"),
            ("REPOSITORY", "${{ github.repository }}"),
        ]
        .iter()
        .all(|(key, value)| env.get(*key).is_some_and(|actual| actual == value))
}

// Admit only a mandatory top-level success guard; this does not evaluate expressions.
fn successful_dependency(condition: Option<&str>, proof: &str) -> bool {
    let Some(condition) = condition else {
        return true;
    };
    let guard = format!("needs.{proof}.result == 'success' && ");
    if !condition.starts_with(&guard) && !condition.starts_with(&format!("always() && {guard}")) {
        return false;
    }
    let mut depth = 0_u32;
    let mut quoted = false;
    let mut chars = condition.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            quoted = !quoted;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' => {
                let Some(next) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next;
            }
            '|' if depth == 0 && chars.peek() == Some(&'|') => return false,
            _ => {}
        }
    }
    depth == 0 && !quoted
}

#[cfg(test)]
#[path = "action_credentials_tests.rs"]
mod tests;
