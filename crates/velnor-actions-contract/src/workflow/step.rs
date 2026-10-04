//! Explicit step identities and payload validation.

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One workflow step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Explicit identifier for expression output bindings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<StepId>,
    /// Step name.
    pub name: String,
    /// Run condition (`if`), serialized by the workflow renderer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Step payload.
    #[serde(flatten)]
    pub kind: StepKind,
}
/// Step payload variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    /// Pinned GitHub Action step.
    Action {
        /// Full-SHA `uses` reference.
        uses: String,
        /// Action inputs.
        #[serde(default)]
        with: BTreeMap<String, String>,
        /// Step environment (applies to main and post phases alike).
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Fixed shell argv step.
    Shell {
        /// Fixed argument vector.
        run: Vec<String>,
        /// Fixed environment.
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Owner-qualified generated source with exact compiled-record admission.
    SourceBoundHelper {
        /// Source binding, exact argument vector and managed-tool requirements.
        invocation: super::source_helper::HelperInvocation,
        /// Validated environment supplied by the compiled operation owner.
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Fixed internal planner/aggregation step.
    Internal {
        /// Internal operation name.
        operation: String,
    },
}
impl Step {
    /// Validate one step payload.
    pub(super) fn validate(&self, job: &str) -> Result<(), ContractError> {
        if let Some(id) = &self.id {
            id.validate()?;
        }
        if self.name.trim().is_empty() {
            return Err(ContractError::identity(
                "step.name",
                format!("empty_name:{job}"),
            ));
        }
        if let Some(condition) = &self.condition
            && (condition.trim().is_empty() || condition.bytes().any(|b| b == b'\n' || b == b'\r'))
        {
            return Err(ContractError::identity(
                "step.condition",
                format!("bad_condition:{job}"),
            ));
        }
        match &self.kind {
            StepKind::Action { uses, .. } => {
                if uses.trim().is_empty() {
                    return Err(ContractError::identity(
                        "step.uses",
                        format!("empty_uses:{job}"),
                    ));
                }
            }
            StepKind::Shell { run, .. } => {
                if run.is_empty() || run.iter().any(|arg| arg.trim().is_empty()) {
                    return Err(ContractError::identity(
                        "step.run",
                        format!("bad_argv:{job}"),
                    ));
                }
            }
            StepKind::Internal { operation } => {
                if operation.trim().is_empty() {
                    return Err(ContractError::identity(
                        "step.operation",
                        format!("empty_operation:{job}"),
                    ));
                }
            }
            StepKind::SourceBoundHelper { invocation, .. } => invocation.validate()?,
        }
        Ok(())
    }
}
/// Validated GitHub Actions step identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StepId(String);

impl StepId {
    /// Construct an identifier: ASCII letter/underscore followed by letters,
    /// digits, underscores, or hyphens.
    /// # Errors
    /// Rejects identifiers outside the GitHub Actions identifier grammar.
    pub fn new(value: &str) -> Result<Self, ContractError> {
        let id = Self(value.to_owned());
        id.validate()?;
        Ok(id)
    }

    /// Borrow the identifier for expression bindings and YAML.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate the identifier grammar.
    /// # Errors
    /// Rejects empty identifiers and invalid characters.
    pub fn validate(&self) -> Result<(), ContractError> {
        let first = self.0.bytes().next();
        if !first.is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
            || !self
                .0
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(ContractError::identity(
                "step.id",
                format!("bad_step_id:{}", self.0),
            ));
        }
        Ok(())
    }
}

impl TryFrom<String> for StepId {
    type Error = ContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

impl From<StepId> for String {
    fn from(value: StepId) -> Self {
        value.0
    }
}

/// Reject duplicate output bindings within a job.
/// # Errors
/// Rejects repeated or malformed step identifiers.
pub fn validate_step_ids(steps: &[Step]) -> Result<(), ContractError> {
    let mut ids = BTreeSet::new();
    for id in steps.iter().filter_map(|step| step.id.as_ref()) {
        id.validate()?;
        if !ids.insert(id.as_str()) {
            return Err(ContractError::identity(
                "step.id",
                format!("duplicate_step_id:{}", id.as_str()),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Step, StepId, StepKind, validate_step_ids};

    #[test]
    fn identifiers_validate_on_construction_and_deserialization() {
        for value in ["cache", "velnor-sources-cache", "_step1", "Build_2"] {
            let id = StepId::new(value).expect("valid identifier");
            assert_eq!(id.as_str(), value);
            let encoded = serde_json::to_string(&id).expect("serialize");
            let decoded: StepId = serde_json::from_str(&encoded).expect("deserialize");
            assert_eq!(id, decoded);
        }
        for value in ["", "1step", "a.b", "a b", "a\n", "é"] {
            assert!(StepId::new(value).is_err(), "{value:?}");
            let encoded = serde_json::to_string(value).expect("serialize");
            assert!(
                serde_json::from_str::<StepId>(&encoded).is_err(),
                "{value:?}"
            );
        }
    }

    #[test]
    fn absent_ids_are_optional_but_duplicate_bindings_fail() {
        let mut step = Step {
            id: None,
            name: "run".to_owned(),
            condition: None,
            kind: StepKind::Internal {
                operation: "plan".to_owned(),
            },
        };
        assert!(validate_step_ids(&[step.clone(), step.clone()]).is_ok());
        let json = serde_json::to_value(&step).expect("serialize");
        assert!(json.get("id").is_none());
        step.id = Some(StepId::new("build").expect("identifier"));
        let error = validate_step_ids(&[step.clone(), step]).expect_err("duplicate");
        assert!(error.to_string().contains("duplicate_step_id:build"));
    }
}
