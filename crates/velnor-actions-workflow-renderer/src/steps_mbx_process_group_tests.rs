use std::collections::BTreeMap;

use velnor_actions_contract::{MbxProcessGroupOperation, MbxProcessGroupShell, Step, StepKind};

use crate::render::RenderContext;

use super::{expected_base_env, mbx_process_group_shell_step, rendered_run, validate_step};

fn render_context() -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: "actions/checkout@0000000000000000000000000000000000000000".to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: BTreeMap::new(),
    }
}

#[test]
fn operation_payload_renders_only_the_fixed_bash_vector() {
    for operation in [
        MbxProcessGroupOperation::Import,
        MbxProcessGroupOperation::Export,
    ] {
        let step_result = mbx_process_group_shell_step(operation, &expected_base_env(operation));
        assert!(step_result.is_ok());
        let Ok(step) = step_result else {
            return;
        };
        assert!(matches!(&step.kind, StepKind::MbxProcessGroupShell(_)));
        let StepKind::MbxProcessGroupShell(payload) = &step.kind else {
            return;
        };
        let run = rendered_run(&step, payload);
        assert!(run.is_ok(), "{run:?}");
        let Ok(run) = run else {
            return;
        };
        assert_eq!(run.first().map(String::as_str), Some("bash"));
        assert_eq!(run.get(1).map(String::as_str), Some("-c"));
        let Some(body) = run.get(2) else {
            return;
        };
        assert!(body.contains("mbx \"$@\" & mbx_pid=$!"));
        let operation_call = match operation {
            MbxProcessGroupOperation::Import => "run_mbx cache import \"$bundle\"",
            MbxProcessGroupOperation::Export => {
                "run_mbx cache export --group \"$group\" --format directory \"$bundle\""
            }
        };
        assert!(body.contains(operation_call));
        let yaml = crate::document_steps::step_to_yaml(
            "probe",
            &step,
            &render_context(),
            &[],
            false,
            &BTreeMap::new(),
        );
        assert!(yaml.is_ok());
        if let Ok(yaml) = yaml {
            let rendered = crate::yaml::render_yaml(&yaml);
            assert!(rendered.contains("bash -c"));
            let yaml_launch = r#"mbx \"$@\" & mbx_pid=$!"#;
            assert!(rendered.contains(yaml_launch));
            let yaml_operation_call = operation_call.replace('"', "\\\"");
            assert!(rendered.contains(&yaml_operation_call));
        }
    }
}

#[test]
fn altered_fixed_operation_body_is_rejected() {
    let mut run = crate::stock_mbx_bundle::process_group_run(MbxProcessGroupOperation::Import);
    let Some(script) = run.get_mut(2) else {
        return;
    };
    script.push_str("; $(arbitrary_command)");
    assert!(super::validate_fixed_script(MbxProcessGroupOperation::Import, &run).is_err());
}

#[test]
fn typed_payload_rejects_outer_step_mutation_and_extra_environment() {
    let mut env = expected_base_env(MbxProcessGroupOperation::Import);
    env.insert("UNAPPROVED".to_owned(), "value".to_owned());
    let payload = MbxProcessGroupShell::import(crate::toolchain_env::with_credential_scrub(&env));
    let step = Step {
        name: "Import MBX single bundle".to_owned(),
        condition: None,
        continue_on_error: None,
        kind: StepKind::MbxProcessGroupShell(payload.clone()),
    };
    assert!(validate_step(&step, &payload).is_err());
    assert!(
        crate::document_steps::step_to_yaml(
            "probe",
            &step,
            &render_context(),
            &[],
            false,
            &BTreeMap::new(),
        )
        .is_err()
    );
    env.remove("UNAPPROVED");
    let valid = mbx_process_group_shell_step(MbxProcessGroupOperation::Import, &env);
    assert!(valid.is_ok());
    let Ok(mut valid_step) = valid else {
        return;
    };
    let StepKind::MbxProcessGroupShell(valid_payload) = &valid_step.kind else {
        return;
    };
    valid_step.name = "ordinary step".to_owned();
    assert!(validate_step(&valid_step, valid_payload).is_err());
    valid_step.name = "Import MBX single bundle".to_owned();
    valid_step.condition = Some("always()".to_owned());
    assert!(validate_step(&valid_step, valid_payload).is_err());
    valid_step.condition = None;
    valid_step.continue_on_error = Some(true);
    assert!(validate_step(&valid_step, valid_payload).is_err());
}

#[test]
fn same_named_generic_shell_cannot_use_process_group_authority() {
    let generated = mbx_process_group_shell_step(
        MbxProcessGroupOperation::Import,
        &expected_base_env(MbxProcessGroupOperation::Import),
    );
    assert!(generated.is_ok());
    let Ok(mut step) = generated else {
        return;
    };
    step.kind = StepKind::Shell {
        run: vec![
            "bash".to_owned(),
            "-c".to_owned(),
            "set -m; mbx \"$@\" & mbx_pid=$!".to_owned(),
        ],
        env: BTreeMap::new(),
    };
    assert!(
        crate::document_steps::step_to_yaml(
            "probe",
            &step,
            &render_context(),
            &[],
            false,
            &BTreeMap::new(),
        )
        .is_err()
    );
}
