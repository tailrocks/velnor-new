use super::*;

pub(super) fn bind_id(owner_id: &str) -> String {
    QualificationCacheStepId::new(
        QualificationCacheStepStage::Bind,
        QualificationCacheLayer::CargoSources,
        owner_id,
    )
    .expect("valid bind identity")
    .as_string()
}

pub(super) fn task_step(bind: &str) -> Step {
    Step {
        name: "Run task".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::TaskExecution {
            argv: vec!["cargo".to_owned(), "test".to_owned()],
            env: BTreeMap::from([(
                "TASK_NOTE".to_owned(),
                format!("steps.{bind}.outputs.restore_key"),
            )]),
            task_id: "rust-demo-test".to_owned(),
            task_digest: crate::workflow::TaskBindingDigest::new(format!("b3-{}", "a".repeat(64)))
                .expect("valid task binding"),
            toolchain_inputs: crate::cachekey::ToolchainInputs {
                tools: vec!["rust@1.99.0".to_owned()],
                components: vec!["clippy".to_owned()],
                compile_driver: "cargo".to_owned(),
                test_runner: "cargo_test".to_owned(),
            },
            matrix_id: "rust-demo-test-matrix".to_owned(),
            matrix_key: "rust-demo-test".to_owned(),
            report_helper_version: "0.1.0".to_owned(),
            matrix_max_parallel: None,
        },
    }
}

pub(super) fn literal_shell_step(bind: &str) -> Step {
    Step {
        name: "Unrelated literal data".to_owned(),
        id: None,
        role: None,
        condition: Some(format!(
            "contains('steps.{bind}.outcome', steps.{bind}.outcome)"
        )),
        parallel: false,
        kind: StepKind::Shell {
            run: vec![
                format!("echo steps.{bind}.outputs.restore_key"),
                format!("echo ${{{{ 'steps.{bind}.outputs.restore_key' }}}}"),
                format!("echo ${{{{ steps.{bind}.outputs.restore_key }}}}"),
                format!("echo ${{{{ format('}}}}', steps.{bind}.outputs.restore_key) }}}}"),
                format!("echo ${{{{ github.event.steps.{bind}.outputs.restore_key }}}}"),
            ],
            env: BTreeMap::from([
                (
                    "LITERAL".to_owned(),
                    format!("steps.{bind}.outputs.restore_key"),
                ),
                (
                    "EXPRESSION_LITERAL".to_owned(),
                    format!("${{{{ 'steps.{bind}.outputs.restore_key' }}}}"),
                ),
                (
                    "EXPRESSION_REFERENCE".to_owned(),
                    format!("${{{{ steps.{bind}.outputs.restore_key }}}}"),
                ),
            ]),
        },
    }
}

pub(super) fn literal_action_step(bind: &str) -> Step {
    Step {
        name: "Unrelated action data".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: "actions/example@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            with: BTreeMap::from([
                (
                    "literal".to_owned(),
                    format!("steps.{bind}.outputs.restore_key"),
                ),
                (
                    "expression_literal".to_owned(),
                    format!("${{{{ 'steps.{bind}.outputs.restore_key' }}}}"),
                ),
                (
                    "expression_reference".to_owned(),
                    format!("${{{{ steps.{bind}.outputs.restore_key }}}}"),
                ),
            ]),
            env: BTreeMap::from([(
                "LITERAL".to_owned(),
                format!("${{{{ 'steps.{bind}.outputs.restore_key' }}}}"),
            )]),
        },
    }
}

pub(super) fn assert_literal_shell(step: &Step, bind: &str, hosted_bind: &str) {
    let StepKind::Shell { run, env } = &step.kind else {
        panic!("the unrelated shell step must remain in place");
    };
    assert_eq!(run[0], format!("echo steps.{bind}.outputs.restore_key"));
    assert_eq!(
        run[1],
        format!("echo ${{{{ 'steps.{bind}.outputs.restore_key' }}}}")
    );
    assert_eq!(
        run[2],
        format!("echo ${{{{ steps.{hosted_bind}.outputs.restore_key }}}}")
    );
    assert_eq!(
        run[3],
        format!("echo ${{{{ format('}}}}', steps.{hosted_bind}.outputs.restore_key) }}}}")
    );
    assert_eq!(
        run[4],
        format!("echo ${{{{ github.event.steps.{bind}.outputs.restore_key }}}}")
    );
    assert_eq!(
        step.condition.as_deref(),
        Some(format!("contains('steps.{bind}.outcome', steps.{hosted_bind}.outcome)").as_str())
    );
    assert_eq!(env["LITERAL"], format!("steps.{bind}.outputs.restore_key"));
    assert_eq!(
        env["EXPRESSION_LITERAL"],
        format!("${{{{ 'steps.{bind}.outputs.restore_key' }}}}")
    );
    assert_eq!(
        env["EXPRESSION_REFERENCE"],
        format!("${{{{ steps.{hosted_bind}.outputs.restore_key }}}}")
    );
}

pub(super) fn assert_literal_action(step: &Step, bind: &str, hosted_bind: &str) {
    let StepKind::Action { with, env, .. } = &step.kind else {
        panic!("the unrelated action step must remain in place");
    };
    assert_eq!(with["literal"], format!("steps.{bind}.outputs.restore_key"));
    assert_eq!(
        with["expression_literal"],
        format!("${{{{ 'steps.{bind}.outputs.restore_key' }}}}")
    );
    assert_eq!(
        with["expression_reference"],
        format!("${{{{ steps.{hosted_bind}.outputs.restore_key }}}}")
    );
    assert_eq!(
        env["LITERAL"],
        format!("${{{{ 'steps.{bind}.outputs.restore_key' }}}}")
    );
}

pub(super) fn literal_internal_step(bind: &str) -> Step {
    Step {
        name: "Unrelated internal data".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: "literal-data-test".to_owned(),
            env: BTreeMap::from([
                ("LITERAL".to_owned(), format!("steps.{bind}.outputs.result")),
                (
                    "EXPRESSION_LITERAL".to_owned(),
                    format!("${{{{ 'steps.{bind}.outputs.result' }}}}"),
                ),
                (
                    "EXPRESSION_REFERENCE".to_owned(),
                    format!("${{{{ steps.{bind}.outputs.result }}}}"),
                ),
            ]),
        },
    }
}

pub(super) fn assert_literal_internal(step: &Step, bind: &str, hosted_bind: &str) {
    let StepKind::Internal { env, .. } = &step.kind else {
        panic!("the unrelated internal step must remain in place");
    };
    assert_eq!(env["LITERAL"], format!("steps.{bind}.outputs.result"));
    assert_eq!(
        env["EXPRESSION_LITERAL"],
        format!("${{{{ 'steps.{bind}.outputs.result' }}}}")
    );
    assert_eq!(
        env["EXPRESSION_REFERENCE"],
        format!("${{{{ steps.{hosted_bind}.outputs.result }}}}")
    );
}
