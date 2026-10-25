use std::collections::BTreeMap;

use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepId, StepKind, StepRole, Trigger,
    WorkflowIr,
};
use velnor_actions_mise::ToolCatalog;

use super::{VerifierActionSet, call_step, verifier_steps};

fn fixtures() -> (VerifierActionSet, super::VerifierSteps) {
    let catalog = ToolCatalog::pinned().for_emission();
    let actions = VerifierActionSet::build(&catalog, env!("CARGO_PKG_VERSION"))
        .expect("canonical verifier action set");
    let steps = verifier_steps(&catalog).expect("canonical verifier shell steps");
    (actions, steps)
}

fn workflow(steps: Vec<Step>) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs: BTreeMap::from([(
            "verify".to_owned(),
            Job {
                display_name: "Verify".to_owned(),
                runs_on: "ubuntu-26.04".to_owned(),
                check_runner: None,
                timeout_minutes: JobTimeout::VALIDATOR,
                needs: Vec::new(),
                condition: None,
                permissions: None,
                cache_mode: None,
                environment: None,
                services: Vec::new(),
                steps,
            },
        )]),
    }
}

#[test]
fn workflow_lowering_selects_each_composite_by_role_and_full_payload() {
    let (actions, steps) = fixtures();
    let mut source_steps = vec![steps.restored_rust, steps.installed_rust, steps.jactionlint];
    for step in &mut source_steps {
        step.name = "Neutral verifier display label".to_owned();
    }

    let mut ir = workflow(source_steps);
    actions
        .lower_workflow_calls(&mut ir, &[])
        .expect("canonical verifier calls lower");
    let lowered = &ir.jobs["verify"].steps;
    for (step, action) in lowered.iter().zip([
        &actions.restored_rust,
        &actions.installed_rust,
        &actions.jactionlint,
    ]) {
        assert_eq!(step.name, "Neutral verifier display label");
        assert!(VerifierActionSet::canonical_call(step, action));
    }
    assert_eq!(
        match &lowered[0].kind {
            StepKind::Action { uses, .. } => uses.as_str(),
            _ => panic!("restored verifier lowers to its local action"),
        },
        actions.restored_rust.uses,
        "restored phase stays distinct from the installed phase"
    );
}

#[test]
fn canonical_alternate_rust_verifiers_remain_inline_and_jactionlint_lowers() {
    let (actions, steps) = fixtures();
    let mut default_home = steps.default_home_rust;
    default_home.name = "Neutral default-home label".to_owned();
    let mut producer_home = steps.producer_home_rust;
    producer_home.name = "Neutral producer-home label".to_owned();
    let mut inline_jactionlint = steps.inline_jactionlint;
    inline_jactionlint.name = "Neutral inline jactionlint label".to_owned();
    let mut ir = workflow(vec![default_home, producer_home, inline_jactionlint]);

    actions
        .lower_workflow_calls(&mut ir, &[])
        .expect("supported alternate verifier contexts remain valid");
    let lowered = &ir.jobs["verify"].steps;
    assert!(matches!(&lowered[0].kind, StepKind::Shell { .. }));
    assert!(matches!(&lowered[1].kind, StepKind::Shell { .. }));
    assert!(matches!(&lowered[2].kind, StepKind::Action { .. }));
    assert!(VerifierActionSet::canonical_call(
        &lowered[2],
        &actions.jactionlint
    ));
    assert_eq!(lowered[0].name, "Neutral default-home label");
    assert_eq!(lowered[1].name, "Neutral producer-home label");
    assert_eq!(lowered[2].name, "Neutral inline jactionlint label");
}

#[test]
fn inline_audit_template_uses_project_run_reference_shell_quoting() {
    let (actions, steps) = fixtures();
    let template = &actions.inline_installed_rust_templates[0];
    let rendered =
        velnor_actions_workflow_renderer::verifier_composite::render_verifier_workflow_step(
            &steps.installed_rust,
            &BTreeMap::new(),
        )
        .expect("canonical inline Rust verifier mapping");
    let velnor_actions_workflow_renderer::yaml::Yaml::Map(fields) = rendered else {
        panic!("verifier step mapping");
    };
    let raw_run = fields
        .iter()
        .find_map(|(key, value)| (key == "run").then_some(value))
        .and_then(|value| match value {
            velnor_actions_workflow_renderer::yaml::Yaml::Str(value)
            | velnor_actions_workflow_renderer::yaml::Yaml::Quoted(value) => Some(value),
            _ => None,
        })
        .expect("rendered run scalar");
    let quoted = velnor_actions_workflow_renderer::quote_project_tool_run_refs(raw_run)
        .expect("project tool references have balanced shell quoting");
    let expected = velnor_actions_workflow_renderer::yaml::render_yaml(
        &velnor_actions_workflow_renderer::yaml::Yaml::Map(vec![(
            "run".to_owned(),
            velnor_actions_workflow_renderer::yaml::Yaml::str(quoted),
        )]),
    );
    let expected = expected
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("run: "))
        .expect("rendered run field")
        .to_owned();
    assert_eq!(template.run_scalar, expected);
}

#[test]
fn verifier_role_with_changed_payload_or_metadata_fails_closed() {
    let (actions, steps) = fixtures();
    let canonical = steps.installed_rust;
    let mutations: [fn(&mut Step); 6] = [
        |step| {
            let StepKind::Shell { run, .. } = &mut step.kind else {
                panic!("shell fixture");
            };
            run[2].push_str(" # changed");
        },
        |step| {
            let StepKind::Shell { env, .. } = &mut step.kind else {
                panic!("shell fixture");
            };
            env.insert("EXTRA".to_owned(), "value".to_owned());
        },
        |step| step.id = Some(StepId::CacheKeys),
        |step| step.condition = Some("always()".to_owned()),
        |step| step.parallel = true,
        |step| step.role = Some(StepRole::VerifyJactionlint),
    ];

    for mutate in mutations {
        let mut changed = canonical.clone();
        mutate(&mut changed);
        let mut ir = workflow(vec![changed]);
        let error = actions
            .lower_workflow_calls(&mut ir, &[])
            .expect_err("modified verifier role cannot fall back to inline shell");
        assert!(
            error
                .to_string()
                .contains("noncanonical_generated_verifier_shell_step"),
            "{error}"
        );
    }
}

#[test]
fn verifier_action_reference_and_role_must_match_in_both_directions() {
    let (actions, _) = fixtures();
    let reversed_rust = call_step(
        "Neutral verifier label",
        actions.restored_rust.uses,
        Some(StepRole::VerifyJactionlint),
    );
    assert!(!VerifierActionSet::canonical_call(
        &reversed_rust,
        &actions.restored_rust
    ));
    assert!(!VerifierActionSet::canonical_call(
        &reversed_rust,
        &actions.jactionlint
    ));

    let reversed_jactionlint = call_step(
        "Neutral verifier label",
        actions.jactionlint.uses,
        Some(StepRole::VerifyRustManifest),
    );
    assert!(!VerifierActionSet::canonical_call(
        &reversed_jactionlint,
        &actions.restored_rust
    ));
    assert!(!VerifierActionSet::canonical_call(
        &reversed_jactionlint,
        &actions.jactionlint
    ));
}
