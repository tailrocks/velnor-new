#[test]
fn rendered_action_wires_seed_admission_and_unset_miss_outputs() {
    let file = super::super::action_file("0.1.0", &[]).expect("restore action");
    for expected in [
        "seed-admitted:",
        "default: \"false\"",
        "TOOLS_SEED_ADMITTED: ${{ inputs.seed-admitted }}",
        "cache_hit=\\\"${TOOLS_CACHE_HIT-}\\\"",
        "matched_key=\\\"${TOOLS_MATCHED_KEY-}\\\"",
        "elif [ -z \\\"$cache_hit\\\" ]",
        "[ \\\"$seed_admitted\\\" = true ]",
        "mise-sha256:",
        "TOOLS_MISE_SHA256: ${{ inputs.mise-sha256 }}",
    ] {
        assert!(file.bytes.contains(expected), "missing {expected}");
    }
}

#[test]
fn generated_action_tree_scopes_seed_input_to_the_restore_composite() {
    use crate::yaml::Yaml;

    fn field<'a>(node: &'a Yaml, key: &str) -> &'a Yaml {
        let Yaml::Map(entries) = node else {
            panic!("expected mapping containing {key}");
        };
        entries
            .iter()
            .find_map(|(name, value)| (name == key).then_some(value))
            .unwrap_or_else(|| panic!("missing mapping field {key}"))
    }

    let body = super::super::action_body(&[]).expect("restore action tree");
    let seed_input = field(field(&body, "inputs"), "seed-admitted");
    assert_eq!(field(seed_input, "required"), &Yaml::Bool(false));
    assert_eq!(
        field(seed_input, "default"),
        &Yaml::Quoted("false".to_owned())
    );

    let Yaml::Seq(steps) = field(field(&body, "runs"), "steps") else {
        panic!("composite steps are a sequence");
    };
    assert_eq!(field(&steps[0], "id"), &Yaml::Str("restore".to_owned()));
    let admission_env = field(&steps[1], "env");
    assert_eq!(
        field(admission_env, "TOOLS_SEED_ADMITTED"),
        &Yaml::Str("${{ inputs.seed-admitted }}".to_owned())
    );
}

#[test]
fn restore_call_requires_the_exact_prelude_output_input() {
    let mut wrong = restore_step();
    let velnor_actions_contract::StepKind::Action { with, .. } = &mut wrong.kind else {
        panic!("restore is an action");
    };
    with.insert(
        crate::cache_steps::TOOLS_SEED_ADMITTED_INPUT.to_owned(),
        "caller-value".to_owned(),
    );
    assert!(super::super::validate_call(&wrong).is_err());

    let mut missing = restore_step();
    let velnor_actions_contract::StepKind::Action { with, .. } = &mut missing.kind else {
        panic!("restore is an action");
    };
    with.remove(crate::cache_steps::TOOLS_SEED_ADMITTED_INPUT);
    assert!(super::super::validate_call(&missing).is_err());
}

#[test]
fn relocated_restore_accepts_only_the_exact_plan_input_condition() {
    let mut relocated = restore_step();
    relocated.condition =
        Some(crate::cache_p08::TOOLS_CACHE_RELOCATED_RESTORE_CONDITION.to_owned());
    assert!(super::super::validate_call(&relocated).is_err());
    assert_eq!(
        super::super::validate_call_for_relocated_child(&relocated)
            .expect("exact relocated restore condition is accepted"),
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION
    );

    let mut event = restore_step();
    event.condition = Some(crate::cache_p08::TOOLS_CACHE_PLAN_RESTORE_CONDITION.to_owned());
    assert!(super::super::validate_call_for_relocated_child(&event).is_ok());

    for condition in [
        "inputs.plan_result == 'failure' && inputs.qualification_campaign == '' && steps.v2.outputs.enabled == 'true'",
        "inputs.plan_result == 'success' && inputs.qualification_campaign != '' && steps.v2.outputs.enabled == 'true'",
        "inputs.plan_result == 'success' && inputs.qualification_campaign == '' && steps.v2.outputs.enabled == 'false'",
        "inputs.plan_result == 'success' && inputs.qualification_campaign == '' && steps.v2.outputs.enabled == 'true' && always()",
        "inputs.plan_result == 'success' && inputs.qualification_campaign == ''",
    ] {
        let mut malformed = restore_step();
        malformed.condition = Some(condition.to_owned());
        assert!(
            super::super::validate_call_for_relocated_child(&malformed).is_err(),
            "accepted malformed restore condition: {condition}"
        );
    }
}

fn restore_step() -> velnor_actions_contract::Step {
    crate::cache_steps::tools_cache_restore_step(
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
        &"a".repeat(64),
        Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION.to_owned()),
    )
    .expect("typed restore step")
}
