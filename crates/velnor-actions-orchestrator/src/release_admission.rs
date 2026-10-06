//! Common exact-source CI admission helper, independent of delivery domains.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    CompiledSourceHelper, CompiledSupportSource, ContractError, HelperInvocation,
    SourceBoundHelper, SourceBoundOperation,
    workflow::native_tools::{CompiledNativeExecRecipe, NativeCredentialScope},
};
use velnor_actions_mise::catalog::qualification::DistributionHost;
use velnor_actions_workflow_renderer::{RenderError, release_spec::validate_repository};

use crate::OrchestratorError;

/// Fixed generated admission helper path in the protected policy checkout.
pub(crate) const ADMISSION_PATH: &str = ".github/velnor/release_admission.py";

/// Closed caller policy for the embedded admission executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionMode {
    /// Admission from a protected default branch.
    DefaultBranch,
}

impl AdmissionMode {
    const fn event_policy(self) -> &'static str {
        match self {
            Self::DefaultBranch => "default-branch",
        }
    }

    const fn ref_kind(self) -> &'static str {
        match self {
            Self::DefaultBranch => "branch",
        }
    }
}

/// Approved source SHA binding for the executable's typed identity.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceSha {
    /// Exact generation-time source identity.
    Approved(String),
    /// The immutable source SHA supplied by the GitHub event.
    GithubSha,
}

/// Repository, branch, and source identity admitted by the compiled owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AdmissionIdentity {
    repository: String,
    branch: String,
    source_sha: SourceSha,
}

impl AdmissionIdentity {
    /// Construct a checked identity; callers cannot supply a policy or argv.
    /// # Errors
    /// Rejects malformed repository, branch, or literal source identity.
    fn new(repository: &str, branch: &str, source_sha: SourceSha) -> Result<Self, RenderError> {
        validate_repository(repository)?;
        if !velnor_actions_contract::is_valid_branch_name(branch) {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_admission_branch:{branch}"
            )));
        }
        if let SourceSha::Approved(value) = &source_sha {
            velnor_actions_workflow_renderer::release_spec::validate_source_sha(value)?;
        }
        Ok(Self {
            repository: repository.to_owned(),
            branch: branch.to_owned(),
            source_sha,
        })
    }

    /// Construct an identity bound to the current GitHub event source.
    /// # Errors
    /// Rejects malformed repository or branch identity.
    fn github_sha(repository: &str, branch: &str) -> Result<Self, RenderError> {
        Self::new(repository, branch, SourceSha::GithubSha)
    }

    /// Construct an identity bound to one approved literal source SHA.
    /// # Errors
    /// Rejects malformed repository, branch, or source SHA.
    pub(super) fn approved(
        repository: &str,
        branch: &str,
        source_sha: &str,
    ) -> Result<Self, RenderError> {
        Self::new(
            repository,
            branch,
            SourceSha::Approved(source_sha.to_owned()),
        )
    }

    /// Approved repository slug.
    #[must_use]
    fn repository(&self) -> &str {
        &self.repository
    }

    /// Approved default branch.
    #[must_use]
    fn branch(&self) -> &str {
        &self.branch
    }
}

/// Build the exact embedded admission executable from the SDK catalog.
/// # Errors
/// Fails closed when the host or credential scope lacks an exact SDK recipe.
pub(super) fn executable(
    mode: AdmissionMode,
    identity: &AdmissionIdentity,
    generator_version: &str,
    host: DistributionHost,
    scope: NativeCredentialScope,
) -> Result<CompiledSourceHelper, RenderError> {
    validate_scope(scope)?;
    let recipe = admission_recipe(host, generator_version)?;
    compiled_record(mode, identity, generator_version, recipe)
}

/// Reconstruct the complete Python-Full/GitHub-Planning admission envelope.
/// # Errors
/// Fails closed when Mise, either measured host tool, or any SDK authority is missing.
fn admission_recipe(
    host: DistributionHost,
    generator_version: &str,
) -> Result<CompiledNativeExecRecipe, RenderError> {
    let tools =
        velnor_actions_mise::catalog::delivery_tools::admission_tools(host, generator_version)
            .map_err(sdk)?;
    velnor_actions_mise::catalog::delivery_tools::validate_admission_tools(
        host,
        generator_version,
        &tools,
    )
    .map_err(sdk)?;
    if !tools
        .execution()
        .environment()
        .contains_key(velnor_actions_mise::catalog::delivery_tools::ADMISSION_PLANNING_GH_ENV)
    {
        return Err(RenderError::InvalidWorkflow(
            "release_admission_planning_gh_binding".to_owned(),
        ));
    }
    Ok(tools.execution().clone())
}

#[cfg(test)]
pub(crate) fn fixture_default_branch_admission(
    repository: &str,
    branch: &str,
    generator_version: &str,
    recipe: CompiledNativeExecRecipe,
) -> Result<CompiledSourceHelper, RenderError> {
    recipe.validate().map_err(RenderError::Contract)?;
    let identity = AdmissionIdentity::github_sha(repository, branch)?;
    compiled_record(
        AdmissionMode::DefaultBranch,
        &identity,
        generator_version,
        recipe,
    )
}

fn compiled_record(
    mode: AdmissionMode,
    identity: &AdmissionIdentity,
    generator_version: &str,
    recipe: CompiledNativeExecRecipe,
) -> Result<CompiledSourceHelper, RenderError> {
    let source = executable_source(generator_version)?;
    let operation = SourceBoundOperation::ReleaseAdmissionDefaultBranch;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)
        .map_err(RenderError::Contract)?;
    let invocation = HelperInvocation::compiled(
        descriptor,
        Vec::new(),
        recipe.installed_selectors().to_vec(),
    )
    .map_err(RenderError::Contract)?;
    let record = CompiledSourceHelper::compiled(invocation, source)
        .map_err(RenderError::Contract)?
        .with_environment(environment(mode, identity));
    record
        .with_execution_recipe(recipe)
        .map_err(RenderError::Contract)
}

/// APT/default-branch factory with the fixed read-only GitHub scope.
/// # Errors
/// Rejects malformed identity or a missing exact SDK admission envelope.
pub(crate) fn compiled_default_branch_admission(
    repository: &str,
    branch: &str,
    generator_version: &str,
    host: DistributionHost,
) -> Result<CompiledSourceHelper, RenderError> {
    let identity = AdmissionIdentity::github_sha(repository, branch)?;
    let recipe = admission_recipe(host, generator_version)?;
    compiled_record(
        AdmissionMode::DefaultBranch,
        &identity,
        generator_version,
        recipe,
    )
}

/// Return the SDK-owned admission bootstrap and preparation records.
///
/// The order is fixed: Full Mise bootstrap, Planning Mise bootstrap, Full
/// Python preparation, then Planning GitHub preparation.
/// # Errors
/// Fails closed when the runner label or any measured SDK input is unqualified.
pub(in crate::release_emit) fn admission_tool_records(
    inputs: &super::release_steps::JobInputs<'_>,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let host = host_for_runner_label(inputs.label)?;
    let tools = velnor_actions_mise::catalog::delivery_tools::admission_tools(
        host,
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(admission_sdk_error)?;
    velnor_actions_mise::catalog::delivery_tools::validate_admission_tools(
        host,
        env!("CARGO_PKG_VERSION"),
        &tools,
    )
    .map_err(admission_sdk_error)?;
    if !tools
        .execution()
        .environment()
        .contains_key(velnor_actions_mise::catalog::delivery_tools::ADMISSION_PLANNING_GH_ENV)
    {
        return Err(OrchestratorError::Contract {
            problem: "release_admission_tools:planning_gh_binding_missing".to_owned(),
        });
    }
    Ok(tools
        .bootstrap_records()
        .iter()
        .chain(tools.preparation_records().iter())
        .cloned()
        .collect())
}

fn host_for_runner_label(label: &str) -> Result<DistributionHost, OrchestratorError> {
    match velnor_actions_contract::tool_target_for_runner_label(label) {
        Some("x86_64-unknown-linux-gnu") => Ok(DistributionHost::LinuxAmd64),
        Some("aarch64-unknown-linux-gnu") => Ok(DistributionHost::LinuxArm64),
        _ => Err(OrchestratorError::Contract {
            problem: "release_admission_host".to_owned(),
        }),
    }
}

fn admission_sdk_error(error: velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("release_admission_tools:{error}"),
    }
}

fn environment(mode: AdmissionMode, identity: &AdmissionIdentity) -> BTreeMap<String, String> {
    let source_sha = match &identity.source_sha {
        SourceSha::Approved(value) => value.clone(),
        SourceSha::GithubSha => "${{ github.sha }}".to_owned(),
    };
    BTreeMap::from([
        (
            "ADMISSION_EVENT_POLICY".to_owned(),
            mode.event_policy().to_owned(),
        ),
        ("ADMISSION_REF_KIND".to_owned(), mode.ref_kind().to_owned()),
        (
            "APPROVED_DEFAULT_BRANCH".to_owned(),
            identity.branch().to_owned(),
        ),
        (
            "APPROVED_REPOSITORY".to_owned(),
            identity.repository().to_owned(),
        ),
        ("APPROVED_SOURCE_SHA".to_owned(), source_sha),
        (
            "GITHUB_EVENT_NAME".to_owned(),
            "${{ github.event_name }}".to_owned(),
        ),
        (
            "GITHUB_REPOSITORY".to_owned(),
            "${{ github.repository }}".to_owned(),
        ),
        (
            "GITHUB_REF_NAME".to_owned(),
            "${{ github.ref_name }}".to_owned(),
        ),
    ])
}

fn executable_source(version: &str) -> Result<String, RenderError> {
    let body = format!(
        r#"set -euo pipefail
if [ "$ADMISSION_REF_KIND" = tag ]; then
    export GITHUB_REF="refs/tags/$GITHUB_REF_NAME"
else
    export GITHUB_REF="refs/heads/$GITHUB_REF_NAME"
fi
exec python3 -I -S - <<'VELNOR_RELEASE_ADMISSION_PY'
{source}
VELNOR_RELEASE_ADMISSION_PY
"#,
        source = source(),
    );
    velnor_actions_contract::generated_source(version, &body).map_err(RenderError::Contract)
}

fn sdk(error: velnor_actions_mise::MiseError) -> RenderError {
    RenderError::InvalidWorkflow(format!("release_admission:{error}"))
}

fn validate_scope(scope: NativeCredentialScope) -> Result<(), RenderError> {
    if scope == NativeCredentialScope::GithubReadOnly {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(
            "release_admission_scope".to_owned(),
        ))
    }
}

/// Authoritative compiled admission helper bytes.
#[must_use]
pub(crate) const fn source() -> &'static str {
    include_str!("release_admission.py")
}

/// Complete generation-only source closure for common CI admission.
/// # Errors
/// Rejects an invalid generator marker version.
pub(super) fn support_sources(version: &str) -> Result<Vec<CompiledSupportSource>, ContractError> {
    Ok(vec![CompiledSupportSource::compiled(
        ADMISSION_PATH,
        source(),
        version,
    )?])
}
