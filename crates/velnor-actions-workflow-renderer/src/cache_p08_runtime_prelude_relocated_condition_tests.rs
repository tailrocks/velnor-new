use velnor_actions_contract::{Step, StepKind};

use super::*;

const RELOCATED_CONDITION: &str = cache_p08::TOOLS_CACHE_RELOCATED_RUN_CONDITION;

#[test]
fn relocated_identity_alias_is_exact_and_keeps_lane_validation() -> Result<(), RenderError> {
    for (lane, source_sha) in [
        ("ubuntu-26.04", None),
        (
            "scale-set:velnor+ubuntu-26.04-scale-set",
            Some("a".repeat(40)),
        ),
    ] {
        let payload = payload_with_source(lane, source_sha.as_deref())?;
        let mut step = payload.runtime_prelude_step()?;
        step.condition = Some(RELOCATED_CONDITION.to_owned());
        let StepKind::Action { uses, with, env } = &step.kind else {
            return Err(RenderError::InvalidWorkflow(
                "relocated_identity_not_action".to_owned(),
            ));
        };

        assert!(cache_p08::validate_runtime_identity_action(&step, uses, lane, with, env).is_err());
        cache_p08::validate_runtime_identity_action_for_relocated_child(
            &step, uses, lane, with, env,
        )?;

        let mut bad_pin = step.clone();
        if let StepKind::Action { uses, .. } = &mut bad_pin.kind {
            *uses = "./.github/actions/u24".to_owned();
        }
        assert!(validate_relocated(&bad_pin, lane).is_err());

        let mut bad_digest = step.clone();
        if let StepKind::Action { with, .. } = &mut bad_digest.kind {
            with.insert(
                cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(),
                "loose".to_owned(),
            );
        }
        assert!(validate_relocated(&bad_digest, lane).is_err());

        let mut bad_env = step.clone();
        if let StepKind::Action { env, .. } = &mut bad_env.kind {
            env.insert("UNEXPECTED".to_owned(), "value".to_owned());
        }
        assert!(validate_relocated(&bad_env, lane).is_err());

        if lane.starts_with("scale-set:") {
            let mut missing_source_sha = step.clone();
            if let StepKind::Action { with, .. } = &mut missing_source_sha.kind {
                with.remove("scale-set-image-source-sha");
            }
            assert!(validate_relocated(&missing_source_sha, lane).is_err());
        }
    }
    Ok(())
}

#[test]
fn relocated_identity_rejects_malformed_input_aliases() -> Result<(), RenderError> {
    for condition in [
        "inputs.plan_result == 'success'",
        "inputs.plan_result == 'success' && inputs.qualification_campaign != ''",
        "inputs.plan_result == 'success' && inputs.qualification_campaign == '' || true",
        "inputs.plan_result == 'success' && inputs.qualification_campaign == '' && steps.v2.outputs.enabled == 'true'",
    ] {
        let payload = payload("ubuntu-26.04")?;
        let mut step = payload.runtime_prelude_step()?;
        step.condition = Some(condition.to_owned());
        assert!(
            validate_relocated(&step, "ubuntu-26.04").is_err(),
            "{condition}"
        );
    }
    Ok(())
}

fn validate_relocated(step: &Step, lane: &str) -> Result<(), RenderError> {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "relocated_identity_not_action".to_owned(),
        ));
    };
    cache_p08::validate_runtime_identity_action_for_relocated_child(step, uses, lane, with, env)
}
