use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, ServiceContainer, Step, StepKind, StepRole};

use super::{CALLER_INPUT_TYPE, CALLER_INPUTS, PROTOTYPE_JOB_ID, pair_is_relocatable, prepare_job};

#[path = "job_workflow_extract_tests/caller_condition.rs"]
mod caller_condition;

fn source_job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Rust CLI hosted".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: vec!["plan".to_owned()],
        condition: Some(
            "${{ needs.plan.result == 'success' && !contains(fromJSON(needs.plan.outputs.not_affected_jobs), 'rust-velnor-actions-cli__hosted') }}"
                .to_owned(),
        ),
        permissions: None,
        cache_mode: None,
        environment: None,
        services: Vec::new(),
        steps,
    }
}

fn action_step() -> Step {
    Step {
        name: "Check ${{ needs.plan.outputs.qualification_campaign }}".to_owned(),
        id: None,
        role: None,
        condition: Some("${{ needs.plan.result == 'success' }}".to_owned()),
        parallel: false,
        kind: StepKind::Action {
            uses: "actions/example@0123456789abcdef0123456789abcdef01234567".to_owned(),
            with: BTreeMap::from([
                (
                    "campaign".to_owned(),
                    "${{ needs.plan.outputs.qualification_campaign }}".to_owned(),
                ),
                (
                    "payload".to_owned(),
                    "{\"source\":\"${{ needs.plan.outputs.sources_key }}\",\"literal\":\"needs.plan.outputs.sources_restore_keys\"}"
                        .to_owned(),
                ),
                (
                    "literal_expression_text".to_owned(),
                    "${{ contains(fromJSON('needs.plan.outputs.not_affected_jobs'), 'rust-cli') }}"
                        .to_owned(),
                ),
                (
                    "quoted_delimiter".to_owned(),
                    "${{ format('}}{0}', needs.plan.result) }}".to_owned(),
                ),
            ]),
            env: BTreeMap::from([(
                "VELNOR_SOURCES_KEY".to_owned(),
                "${{ needs.plan.outputs.sources_key }}".to_owned(),
            )]),
        },
    }
}

fn shell_step() -> Step {
    Step {
        name: "Print source restore key".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["printf '%s' '${{ needs.plan.outputs.sources_restore_keys }}'".to_owned()],
            env: BTreeMap::from([(
                "VELNOR_RUN_KEY_ATTEMPT".to_owned(),
                "${{ needs.plan.outputs.plan_attempt || github.run_attempt }}".to_owned(),
            )]),
        },
    }
}

#[test]
fn keeps_parent_gate_and_rewrites_only_expression_references() {
    let source = source_job(vec![action_step(), shell_step()]);
    let prepared = prepare_job(PROTOTYPE_JOB_ID, &source).expect("closed projection");
    let StepKind::Action { with, .. } = &prepared.child_job.steps[0].kind else {
        panic!("action body remains first");
    };
    let StepKind::Shell { run, env } = &prepared.child_job.steps[1].kind else {
        panic!("shell body remains second");
    };

    assert_eq!(prepared.source_id, PROTOTYPE_JOB_ID);
    assert_eq!(
        prepared.callee_path,
        ".github/workflows/ci-rust-rust-velnor-actions-cli__hosted.yml"
    );
    assert_eq!(prepared.source_job.needs, source.needs);
    assert_eq!(prepared.source_job.condition, source.condition);
    assert_eq!(prepared.child_job.needs, Vec::<String>::new());
    assert!(prepared.child_job.condition.is_none());
    assert_eq!(prepared.child_job.runs_on, source.runs_on);
    assert_eq!(prepared.child_job.timeout_minutes, source.timeout_minutes);
    assert_eq!(prepared.child_job.steps.len(), source.steps.len());
    assert_eq!(
        prepared.child_job.steps[0].condition.as_deref(),
        Some("${{ inputs.plan_result == 'success' }}")
    );
    assert_eq!(
        with.get("campaign").map(String::as_str),
        Some("${{ inputs.qualification_campaign }}")
    );
    assert_eq!(
        with.get("payload").map(String::as_str),
        Some(
            "{\"source\":\"${{ inputs.sources_key }}\",\"literal\":\"needs.plan.outputs.sources_restore_keys\"}"
        )
    );
    assert_eq!(
        with.get("literal_expression_text").map(String::as_str),
        Some("${{ contains(fromJSON('needs.plan.outputs.not_affected_jobs'), 'rust-cli') }}")
    );
    assert_eq!(
        with.get("quoted_delimiter").map(String::as_str),
        Some("${{ format('}}{0}', inputs.plan_result) }}")
    );
    assert_eq!(run[0], "printf '%s' '${{ inputs.sources_restore_keys }}'");
    assert_eq!(
        env.get("VELNOR_RUN_KEY_ATTEMPT").map(String::as_str),
        Some("${{ inputs.run_key_attempt }}")
    );
    assert!(!env["VELNOR_RUN_KEY_ATTEMPT"].contains("github.run_attempt"));
    prepared.validate_source().expect("source-bound projection");
}

#[test]
fn source_bound_projection_rejects_changed_child_body() {
    let mut prepared =
        prepare_job(PROTOTYPE_JOB_ID, &source_job(vec![shell_step()])).expect("closed projection");
    let StepKind::Shell { run, .. } = &mut prepared.child_job.steps[0].kind else {
        panic!("shell body remains first");
    };
    run[0].push_str(" && changed");

    assert!(prepared.validate_source().is_err());
}

#[test]
fn pair_lowerability_uses_the_production_source_preparation_rules() {
    let hosted_id = "rust-velnor-actions-cli__hosted";
    let local_id = "rust-velnor-actions-cli__local";
    let hosted = source_job(vec![shell_step()]);
    let mut local = source_job(vec![shell_step()]);
    local.runs_on = "scale-set:velnor+ubuntu-26.04-scale-set".to_owned();

    assert!(pair_is_relocatable(hosted_id, &hosted, local_id, &local));
    assert!(!pair_is_relocatable(
        hosted_id,
        &hosted,
        "rust-velnor-actions-tofu__local",
        &local,
    ));

    local.needs.push("rust-dependency-seed__hosted".to_owned());
    assert!(!pair_is_relocatable(hosted_id, &hosted, local_id, &local));
}

#[test]
fn rewrites_bare_step_conditions_without_changing_their_form() {
    let mut step = action_step();
    step.condition = Some("always() && needs.plan.result == 'success'".to_owned());
    let prepared =
        prepare_job(PROTOTYPE_JOB_ID, &source_job(vec![step])).expect("closed projection");

    assert_eq!(
        prepared.child_job.steps[0].condition.as_deref(),
        Some("always() && inputs.plan_result == 'success'")
    );
}

#[test]
fn rejects_embedded_attempt_fallbacks_that_change_expression_precedence() {
    let step = Step {
        name: "Attempt in a larger expression".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec![
                "printf '%s' '${{ succeeded() && (needs.plan.outputs.plan_attempt || github.run_attempt) }}'"
                    .to_owned(),
            ],
            env: BTreeMap::new(),
        },
    };
    let error = prepare_job(PROTOTYPE_JOB_ID, &source_job(vec![step]))
        .expect_err("only the complete caller-computed fallback is mapped");

    assert!(error.to_string().contains("unbound_needs_reference"));
}

#[test]
fn exposes_exact_five_string_input_bindings() {
    assert_eq!(CALLER_INPUT_TYPE, "string");
    assert_eq!(
        CALLER_INPUTS.map(|input| input.name),
        [
            "plan_result",
            "qualification_campaign",
            "run_key_attempt",
            "sources_key",
            "sources_restore_keys",
        ]
    );
    assert_eq!(
        CALLER_INPUTS.map(|input| input.expression),
        [
            "${{ needs.plan.result }}",
            "${{ needs.plan.outputs.qualification_campaign }}",
            "${{ needs.plan.outputs.plan_attempt || github.run_attempt }}",
            "${{ needs.plan.outputs.sources_key }}",
            "${{ needs.plan.outputs.sources_restore_keys }}",
        ]
    );
}

#[test]
fn rejects_unmapped_external_needs_references() {
    let unknown = Step {
        name: "Unknown needs output".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["printf '%s' '${{ needs.plan.outputs.new_output }}'".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let error = prepare_job(PROTOTYPE_JOB_ID, &source_job(vec![unknown]))
        .expect_err("unmapped parent context must fail closed");

    assert!(error.to_string().contains("unbound_needs_reference"));
}

#[test]
fn rejects_bracket_and_standalone_needs_context_forms() {
    for expression in [
        "${{ needs['plan'].result }}",
        "${{ toJSON(needs) }}",
        "${{ needs }}",
    ] {
        let step = Step {
            name: "Unsupported context".to_owned(),
            id: None,
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run: vec![format!("printf '%s' '{expression}'")],
                env: BTreeMap::new(),
            },
        };
        let error = prepare_job(PROTOTYPE_JOB_ID, &source_job(vec![step]))
            .expect_err("unsupported needs syntax must fail closed");
        assert!(error.to_string().contains("unbound_needs_reference"));
    }
}

#[test]
fn rejects_unsupported_needs_forms_in_bare_conditions() {
    for condition in [
        "always() && needs['plan'].result == 'success'",
        "toJSON(needs) != ''",
    ] {
        let mut step = action_step();
        step.condition = Some(condition.to_owned());
        let error = prepare_job(PROTOTYPE_JOB_ID, &source_job(vec![step]))
            .expect_err("bare conditions cannot retain unsupported parent context");
        assert!(error.to_string().contains("unbound_needs_reference"));
    }
}

#[test]
fn source_dependencies_must_be_exactly_plan() {
    let mut source = source_job(Vec::new());
    source.needs.push("bootstrap".to_owned());
    let error = prepare_job(PROTOTYPE_JOB_ID, &source)
        .expect_err("additional caller dependencies require a new reviewed contract");

    assert!(
        error
            .to_string()
            .contains("source_dependencies_not_exactly_plan")
    );
}

#[test]
fn dependency_seed_jobs_are_not_relocated() {
    let mut seed = shell_step();
    seed.role = Some(StepRole::MbxSeedBuild);
    let error = prepare_job(PROTOTYPE_JOB_ID, &source_job(vec![seed]))
        .expect_err("dependency-seed failure propagation remains on its parent job");

    assert!(
        error
            .to_string()
            .contains("dependency_seed_not_relocatable")
    );
}

#[test]
fn fixed_job_and_service_fields_cannot_keep_parent_context() {
    let mut source = source_job(Vec::new());
    source
        .services
        .push(ServiceContainer::postgres(18).expect("catalog"));
    source.services[0].options = "--health-cmd '${{ needs.plan.result }}'".to_owned();
    let error = prepare_job(PROTOTYPE_JOB_ID, &source)
        .expect_err("service fields must not retain the removed parent context");

    assert!(
        error
            .to_string()
            .contains("plan_reference_in_fixed_field:services[0].options")
    );
}

#[test]
fn refuses_rewriting_fixed_action_identity_fields() {
    let mut step = action_step();
    let StepKind::Action { uses, .. } = &mut step.kind else {
        unreachable!();
    };
    *uses = "${{ needs.plan.result }}".to_owned();
    let error = prepare_job(PROTOTYPE_JOB_ID, &source_job(vec![step]))
        .expect_err("fixed uses identity cannot contain parent context");

    assert!(error.to_string().contains("plan_reference_in_fixed_field"));
}

#[test]
fn prototype_does_not_extract_a_second_source_job() {
    let error = prepare_job("rust-velnor-actions-mise__hosted", &source_job(Vec::new()))
        .expect_err("single-job prototype remains explicitly closed");

    assert!(error.to_string().contains("outside_single_job_prototype"));
}
