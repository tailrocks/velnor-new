use super::*;

#[test]
fn invalid_or_missing_configured_identity_fails_deserialization() -> Result<(), TransitionError> {
    let plan = plan()?;
    let serialized = serde_json::to_value(&plan).map_err(|_| TransitionError::IncorrectPlan)?;
    let Value::Object(mut fields) = serialized else {
        return Err(TransitionError::IncorrectPlan);
    };
    fields.remove("commit_identity");
    assert!(serde_json::from_value::<TransitionPlan>(Value::Object(fields)).is_err());

    let mut value = serde_json::to_value(plan).map_err(|_| TransitionError::IncorrectPlan)?;
    value["commit_identity"]["name"] = Value::String(String::new());
    assert!(serde_json::from_value::<TransitionPlan>(value).is_err());
    Ok(())
}

#[test]
fn plan_digest_binds_policy_identity_and_target() -> Result<(), TransitionError> {
    let first = plan()?;
    let second = plan()?;
    assert_eq!(first.digest()?, second.digest()?);
    let mut altered = first.clone();
    altered.policy_digest = "3".repeat(64);
    assert_ne!(first.digest()?, altered.digest()?);
    Ok(())
}

#[test]
fn transition_plan_rejects_fork_head() -> Result<(), TransitionError> {
    let mut plan = plan()?;
    plan.head_repository_id += 1;
    assert_eq!(plan.validate(), Err(TransitionError::IncorrectPlan));
    Ok(())
}

#[test]
fn transition_plan_rejects_noop_base_change() -> Result<(), TransitionError> {
    let mut plan = plan()?;
    plan.target_base_ref = plan.previous_base_ref.clone();
    assert_eq!(plan.validate(), Err(TransitionError::IncorrectPlan));
    Ok(())
}
