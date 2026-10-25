use crate::ActionlintError;
use crate::actions::{
    ALINT_ACTION, ALLOWED_ACTIONS, CHECKOUT_ACTION, PinnedActionRef, is_full_sha, is_version_tag,
    split_key,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPinOverride {
    pub action: String,

    pub sha: String,

    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedPin {
    pub sha: String,

    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovedPinCatalog {
    pins: BTreeMap<String, Vec<ApprovedPin>>,
}

impl ApprovedPinCatalog {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        action: &str,
        sha: &str,
        version: &str,
    ) -> Result<(), ActionlintError> {
        if !ALLOWED_ACTIONS.contains(&action) || is_policy_owned(action) {
            return Err(ActionlintError::OverrideRejected {
                action: action.to_owned(),
                problem: "action_not_overridable".to_owned(),
            });
        }
        if !is_full_sha(sha) || !is_version_tag(version) {
            return Err(ActionlintError::OverrideRejected {
                action: action.to_owned(),
                problem: "malformed_pair".to_owned(),
            });
        }
        self.pins
            .entry(action.to_owned())
            .or_default()
            .push(ApprovedPin {
                sha: sha.to_owned(),
                version: version.to_owned(),
            });
        Ok(())
    }

    pub fn validate_override(
        &self,
        request: &ActionPinOverride,
    ) -> Result<PinnedActionRef, ActionlintError> {
        if !ALLOWED_ACTIONS.contains(&request.action.as_str()) {
            return Err(ActionlintError::UnknownAction {
                uses: request.action.clone(),
            });
        }
        if is_policy_owned(&request.action) {
            return Err(ActionlintError::OverrideRejected {
                action: request.action.clone(),
                problem: "action_not_overridable".to_owned(),
            });
        }
        if !is_full_sha(&request.sha) {
            return Err(ActionlintError::OverrideRejected {
                action: request.action.clone(),
                problem: "ref_must_be_full_sha".to_owned(),
            });
        }
        if !is_version_tag(&request.version) {
            return Err(ActionlintError::OverrideRejected {
                action: request.action.clone(),
                problem: format!("invalid_version:{}", request.version),
            });
        }
        let approved = self.pins.get(&request.action).cloned().unwrap_or_default();
        let matched = approved
            .iter()
            .any(|pin| pin.sha == request.sha && pin.version == request.version);
        if !matched {
            return Err(ActionlintError::OverrideRejected {
                action: request.action.clone(),
                problem: "unapproved_pair".to_owned(),
            });
        }
        let (repo, path) = split_key(&request.action);
        PinnedActionRef::new(&repo, path.as_deref(), &request.sha, &request.version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionInputSchema {
    pub action: String,

    pub required: Vec<String>,

    pub optional: Vec<String>,
}

impl ActionInputSchema {
    #[must_use]
    pub fn allowed_inputs(&self) -> BTreeSet<String> {
        self.required
            .iter()
            .chain(self.optional.iter())
            .cloned()
            .collect()
    }
}

#[must_use]
pub fn checkout_inputs_schema() -> ActionInputSchema {
    ActionInputSchema {
        action: CHECKOUT_ACTION.to_owned(),
        required: vec!["persist-credentials".to_owned()],
        optional: vec!["ref".to_owned(), "fetch-depth".to_owned(), "lfs".to_owned()],
    }
}

pub fn validate_action_inputs(
    schema: &ActionInputSchema,
    inputs: &BTreeMap<String, String>,
) -> Result<(), ActionlintError> {
    if !ALLOWED_ACTIONS.contains(&schema.action.as_str()) {
        return Err(ActionlintError::UnknownAction {
            uses: schema.action.clone(),
        });
    }
    let allowed = schema.allowed_inputs();
    for name in inputs.keys() {
        if !allowed.contains(name) {
            return Err(ActionlintError::UnknownActionInput {
                action: schema.action.clone(),
                input: name.clone(),
            });
        }
    }
    for name in &schema.required {
        if !inputs.contains_key(name) {
            return Err(ActionlintError::MissingActionInput {
                action: schema.action.clone(),
                input: name.clone(),
            });
        }
    }
    for (name, value) in inputs {
        check_input_value(&schema.action, name, value)?;
    }
    Ok(())
}

fn is_policy_owned(action: &str) -> bool {
    action == ALINT_ACTION
}

fn check_input_value(action: &str, name: &str, value: &str) -> Result<(), ActionlintError> {
    if value.is_empty() {
        return Err(ActionlintError::InvalidActionInput {
            action: action.to_owned(),
            input: name.to_owned(),
            problem: "empty_value".to_owned(),
        });
    }
    if value.contains('\n') || value.contains('\r') {
        return Err(ActionlintError::InvalidActionInput {
            action: action.to_owned(),
            input: name.to_owned(),
            problem: "multiline_value".to_owned(),
        });
    }
    Ok(())
}
