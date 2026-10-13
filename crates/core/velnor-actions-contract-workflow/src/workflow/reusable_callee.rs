//! Closed V1 contract for a trusted reusable-workflow callee.

use super::permissions::PermissionLevel;
use serde_json::Value;

/// Type of every V1 reusable-workflow input.
pub const REUSABLE_CALLEE_INPUT_TYPE: &str = "string";
/// Exact caller event accepted by the V1 identity guard.
pub const REUSABLE_CALLEE_EVENT: &str = "push";
/// Guard job identifier in the rendered callee.
pub const REUSABLE_CALLEE_GUARD_JOB: &str = "identity-guard";
/// Write-capable job identifier in the rendered callee.
pub const REUSABLE_CALLEE_WRITE_JOB: &str = "trusted-publisher";
/// Exact caller reusable-call permission map for the V1 contract.
pub const REUSABLE_CALLER_PERMISSIONS: [(&str, PermissionLevel); 4] = [
    ("actions", PermissionLevel::Read),
    ("contents", PermissionLevel::Write),
    ("pull-requests", PermissionLevel::None),
    ("id-token", PermissionLevel::None),
];

/// One fixed V1 input and its direct GitHub context expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReusableCalleeInput {
    /// Exact workflow input name.
    pub name: &'static str,
    /// Direct expression forwarded by the trusted caller.
    pub expression: &'static str,
}

impl ReusableCalleeInput {
    const fn new(name: &'static str, expression: &'static str) -> Self {
        Self { name, expression }
    }
}

/// The exact six inputs, in rendered order.
pub const REUSABLE_CALLEE_INPUTS: [ReusableCalleeInput; 6] = [
    ReusableCalleeInput::new("repository", "github.repository"),
    ReusableCalleeInput::new("event_name", "github.event_name"),
    ReusableCalleeInput::new("ref", "github.ref"),
    ReusableCalleeInput::new("sha", "github.sha"),
    ReusableCalleeInput::new("run_id", "github.run_id"),
    ReusableCalleeInput::new("run_attempt", "github.run_attempt"),
];

/// Explicit trust policy used to render and validate the callee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReusableCalleePolicy {
    /// Exact `owner/repository` that may invoke the callee.
    pub caller_repository: String,
    /// Exact repository-relative caller workflow path.
    pub caller_workflow_path: String,
    /// Exact branch containing and publishing the caller workflow.
    pub caller_branch: String,
    /// Exact `owner/repository` that must contain the callee job.
    pub callee_repository: String,
}

impl ReusableCalleePolicy {
    /// Exact full caller workflow reference rendered by V1.
    ///
    /// # Errors
    ///
    /// Returns a stable policy problem when the fixed fields cannot form
    /// the expected GitHub.com `job.workflow_ref` shape.
    pub fn caller_workflow_ref(&self) -> Result<String, ReusableCalleePolicyError> {
        self.validate()?;
        Ok(format!(
            "{}/{}@refs/heads/{}",
            self.caller_repository, self.caller_workflow_path, self.caller_branch
        ))
    }

    /// Reject absent or ambiguous fixed policy fields.
    ///
    /// # Errors
    ///
    /// Returns the first invalid fixed policy field.
    pub fn validate(&self) -> Result<(), ReusableCalleePolicyError> {
        if !exact_repository(&self.caller_repository) || !exact_repository(&self.callee_repository)
        {
            return Err(ReusableCalleePolicyError::Repository);
        }
        if !exact_workflow_path(&self.caller_workflow_path) {
            return Err(ReusableCalleePolicyError::WorkflowPath);
        }
        if !exact_branch(&self.caller_branch) {
            return Err(ReusableCalleePolicyError::Branch);
        }
        Ok(())
    }
}

/// Reasons a reusable-callee policy is absent or ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReusableCalleePolicyError {
    /// A repository field is not the exact GitHub `owner/repository` form.
    Repository,
    /// The caller path is not a normal repository-relative workflow path.
    WorkflowPath,
    /// The branch is empty or contains a GitHub reference separator.
    Branch,
}

/// Runtime GitHub and job identity values checked by the V1 guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReusableCalleeIdentity<'a> {
    /// Calling repository reported by GitHub.
    pub caller_repository: &'a str,
    /// Event reported by GitHub.
    pub event_name: &'a str,
    /// Source ref reported by GitHub.
    pub caller_ref: &'a str,
    /// Full workflow-file reference reported by GitHub.
    pub caller_workflow_ref: &'a str,
    /// Caller workflow commit reported by GitHub.
    pub caller_workflow_sha: &'a str,
    /// Source commit reported by GitHub.
    pub source_sha: &'a str,
    /// Repository containing the current job, reported by GitHub.
    pub callee_repository: &'a str,
}

/// Reasons the V1 identity guard rejects a runtime context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReusableCalleeIdentityError {
    /// Caller repository differs from policy.
    CallerRepository,
    /// The invocation is not a push event.
    EventName,
    /// Source ref differs from policy.
    CallerRef,
    /// Workflow reference differs from policy.
    CallerWorkflowRef,
    /// Workflow and source commit are missing or differ.
    WorkflowSourceSha,
    /// Callee job repository differs from policy.
    CalleeRepository,
}

/// True when the value is a nonempty `owner/repository` without separators.
#[must_use]
pub fn exact_repository(value: &str) -> bool {
    let mut parts = value.split('/');
    let (owner, repository, extra) = (parts.next(), parts.next(), parts.next());
    owner.is_some_and(exact_name) && repository.is_some_and(exact_name) && extra.is_none()
}

/// True for the exact `.github/workflows/<safe-file>.(yaml|yml)` shape.
///
/// Every slash-delimited segment is checked, and the first two segments are
/// fixed so nested directories and path-like metadata cannot enter policy.
#[must_use]
pub fn exact_workflow_path(value: &str) -> bool {
    let mut segments = value.split('/');
    if segments.next() != Some(".github") || segments.next() != Some("workflows") {
        return false;
    }
    if !segments.next().is_some_and(exact_workflow_filename) {
        return false;
    }
    segments.next().is_none()
}

fn exact_workflow_filename(file: &str) -> bool {
    let Some((stem, extension)) = file.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && !file.starts_with('.')
        && !file.ends_with('.')
        && matches!(extension, "yaml" | "yml")
        && file
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// True for a nonempty branch name that cannot change the policy reference.
#[must_use]
pub fn exact_branch(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("//")
        && !value.contains("..")
}

fn exact_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && !value.starts_with('.')
        && !value.ends_with('.')
}

impl ReusableCalleePolicy {
    /// Accept only the exact push caller and callee job identity.
    ///
    /// The caller commit is checked dynamically; it is never embedded in V1.
    ///
    /// # Errors
    ///
    /// Returns the first mismatched identity field.
    pub fn validate_identity(
        &self,
        identity: &ReusableCalleeIdentity<'_>,
    ) -> Result<(), ReusableCalleeIdentityError> {
        if identity.caller_repository != self.caller_repository {
            return Err(ReusableCalleeIdentityError::CallerRepository);
        }
        if identity.event_name != REUSABLE_CALLEE_EVENT {
            return Err(ReusableCalleeIdentityError::EventName);
        }
        if identity.caller_ref != format!("refs/heads/{}", self.caller_branch) {
            return Err(ReusableCalleeIdentityError::CallerRef);
        }
        let caller_workflow_ref = match self.caller_workflow_ref() {
            Ok(workflow_ref) => workflow_ref,
            Err(ReusableCalleePolicyError::WorkflowPath) => {
                return Err(ReusableCalleeIdentityError::CallerWorkflowRef);
            }
            Err(ReusableCalleePolicyError::Repository) => {
                return Err(ReusableCalleeIdentityError::CallerRepository);
            }
            Err(ReusableCalleePolicyError::Branch) => {
                return Err(ReusableCalleeIdentityError::CallerRef);
            }
        };
        if identity.caller_workflow_ref != caller_workflow_ref {
            return Err(ReusableCalleeIdentityError::CallerWorkflowRef);
        }
        if identity.caller_workflow_sha.is_empty()
            || identity.caller_workflow_sha != identity.source_sha
        {
            return Err(ReusableCalleeIdentityError::WorkflowSourceSha);
        }
        if identity.callee_repository != self.callee_repository {
            return Err(ReusableCalleeIdentityError::CalleeRepository);
        }
        Ok(())
    }
}

/// Failure to match the closed six-input schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReusableCalleeContractError {
    /// Schema root is not an object with only `inputs`.
    Root,
    /// `inputs` is missing or is not an object.
    Inputs,
    /// A required input is absent.
    MissingInput(&'static str),
    /// An unknown input name is present.
    UnknownInput(String),
    /// An input is malformed or does not exactly match its fixed declaration.
    InvalidInput(&'static str),
}

impl ReusableCalleeContract {
    /// Parse only the exact closed schema.
    ///
    /// # Errors
    ///
    /// Reports the first closed-schema violation; optional/default fields are
    /// never accepted.
    pub fn from_schema(value: &Value) -> Result<Self, ReusableCalleeContractError> {
        let Value::Object(root) = value else {
            return Err(ReusableCalleeContractError::Root);
        };
        if root.len() != 1 || !matches!(root.get("inputs"), Some(Value::Object(_))) {
            return Err(ReusableCalleeContractError::Root);
        }
        let Value::Object(inputs) = &root["inputs"] else {
            return Err(ReusableCalleeContractError::Inputs);
        };
        for input in REUSABLE_CALLEE_INPUTS {
            validate_input(
                inputs
                    .get(input.name)
                    .ok_or(ReusableCalleeContractError::MissingInput(input.name))?,
            )
            .map_err(|()| ReusableCalleeContractError::InvalidInput(input.name))?;
        }
        for name in inputs.keys() {
            if !REUSABLE_CALLEE_INPUTS
                .iter()
                .any(|input| input.name == name)
            {
                return Err(ReusableCalleeContractError::UnknownInput(name.clone()));
            }
        }
        Ok(Self)
    }
}

/// Validated exact reusable-callee schema marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReusableCalleeContract;

fn validate_input(value: &Value) -> Result<(), ()> {
    let Value::Object(fields) = value else {
        return Err(());
    };
    if fields.len() != 2
        || fields.get("type") != Some(&Value::String(REUSABLE_CALLEE_INPUT_TYPE.to_owned()))
        || fields.get("required") != Some(&Value::Bool(true))
        || fields.contains_key("default")
    {
        return Err(());
    }
    Ok(())
}

/// Canonical closed schema used by the document renderer.
#[must_use]
pub fn schema_value() -> serde_json::Map<String, Value> {
    let inputs = REUSABLE_CALLEE_INPUTS
        .iter()
        .map(|input| {
            (
                input.name.to_owned(),
                Value::Object(
                    [
                        (
                            "type".to_owned(),
                            Value::String(REUSABLE_CALLEE_INPUT_TYPE.to_owned()),
                        ),
                        ("required".to_owned(), Value::Bool(true)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            )
        })
        .collect();
    serde_json::Map::from_iter([("inputs".to_owned(), Value::Object(inputs))])
}
