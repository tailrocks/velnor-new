//! Mutation controls for alternate owners, bindings, tuple shapes, and fetch paths.

use crate::impl_common::TestResult;

use super::{ACTUAL_SOURCE, rejected, replace_once};

fn owner_and_binding_mutations() -> Vec<String> {
    vec![
        replace_once(
            ACTUAL_SOURCE,
            "suite_workflow_files",
            "renamed_suite_workflow_files",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "prep: &GenerationPreparation",
            "config: &GenerationPreparation",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "for suite in &prep.config.workflow.task_suites",
            "for suite in &other.task_suites",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "for suite in &prep.config.workflow.task_suites",
            "for suite in &prep::config::workflow::task_suites",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "Some(input) = &suite.evidence_snapshot",
            "Some(input) = &alias.evidence_snapshot",
        ),
        replace_once(ACTUAL_SOURCE, "let fetch_spec =", "let mut fetch_spec ="),
        replace_once(ACTUAL_SOURCE, "let fetch_spec =", "let ref fetch_spec ="),
        replace_once(ACTUAL_SOURCE, "let fetch_spec =", "let fetch_spec: _ ="),
        replace_once(ACTUAL_SOURCE, "let fetch_spec =", "let fetch_spec @ _ ="),
        replace_once(
            ACTUAL_SOURCE,
            "let fetch_spec =",
            "let fetch_spec if true =",
        ),
        replace_once(ACTUAL_SOURCE, "let fetch_spec =", "let r#fetch_spec ="),
        replace_once(ACTUAL_SOURCE, "Some(input)", "Some(ref input)"),
        replace_once(ACTUAL_SOURCE, "Some(input)", "Some(input @ _)"),
    ]
}

fn resolution_call_mutations() -> Vec<String> {
    vec![
        replace_once(
            ACTUAL_SOURCE,
            "let fetch_spec = input.resolve_spec",
            "let fetch_spec = alias.resolve_spec",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "input.resolve_spec(",
            "input.resolve_spec::<()>(",
        ),
        replace_once(ACTUAL_SOURCE, "&suite.id,", "fetch(&suite.id),"),
        replace_once(ACTUAL_SOURCE, "&suite.id,", "&suite.0,"),
        replace_once(ACTUAL_SOURCE, "&suite.id,", "&suite.r#id,"),
        replace_once(
            ACTUAL_SOURCE,
            "                &prep.default_branch,\n            )?;",
            "            )?;",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "                &prep.default_branch,\n            )?;",
            "                &prep.default_branch,\n                fetch(),\n            )?;",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "                &suite.id,\n                &prep.default_branch,",
            "                &prep.default_branch,\n                &suite.id,",
        ),
    ]
}

fn projection_mutations() -> Vec<String> {
    vec![
        replace_once(
            ACTUAL_SOURCE,
            "Some((fetch_spec, helper))",
            "Some((helper, fetch_spec))",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "Some((fetch_spec, helper))",
            "Some((helper,))",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "Some((fetch_spec, helper))",
            "Some((fetch_spec(), helper))",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "Some((fetch_spec, helper))",
            "Some((fetch_spec, helper, extra))",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "Some((fetch_spec, helper))",
            "Some((fetch_spec, other_helper))",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "evidence_fetch_spec:",
            "evidence_fetch_spec,",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "evidence_fetch_spec: evidence.as_ref()",
            "evidence_fetch_spec: evidence.fetch()",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "evidence_helper: evidence.map",
            "evidence_helper: evidence.fetch().map",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "evidence.as_ref().map(|(spec, _)| spec.clone())",
            "evidence.as_ref().map(|(spec, _)| { fetch(); spec.clone() })",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "evidence.map(|(_, helper)| helper)",
            "evidence.map(|(_, helper)| { fetch(); helper })",
        ),
        replace_once(ACTUAL_SOURCE, "MiseTaskSuitePolicy {", "UnknownPolicy {"),
        replace_once(
            ACTUAL_SOURCE,
            "let fetch_spec = input.resolve_spec(",
            "let fetch_spec = input.resolve_and_fetch_spec(",
        ),
        replace_once(
            ACTUAL_SOURCE,
            "            let helper = crate::pins::consumer_evidence_helper(",
            "            let fetch_spec = input.resolve_spec(\n                &prep.config.workflow.task_suites,\n                &suite.id,\n                &prep.default_branch,\n            )?;\n            let helper = crate::pins::consumer_evidence_helper(",
        ),
    ]
}

fn final_mutations(block_wrapped: String, closure_wrapped: String) -> Vec<String> {
    vec![
        replace_once(
            ACTUAL_SOURCE,
            ")?;\n            let helper =",
            ")? else { unreachable!() };\n            let helper =",
        ),
        block_wrapped,
        closure_wrapped,
        replace_once(
            ACTUAL_SOURCE,
            "        } else {\n            None\n        };",
            "        };",
        )
        .replace("let fetch_spec =", "let r#fetch_spec ="),
        format!("{ACTUAL_SOURCE}\nmacro_rules! hidden {{ () => {{ fetch_spec.fetch() }}; }}"),
    ]
}

#[test]
fn changed_owner_loop_and_binding_shapes_fail_closed() -> TestResult {
    let block_wrapped = replace_once(
        &replace_once(
            ACTUAL_SOURCE,
            "let fetch_spec = input.resolve_spec(",
            "let fetch_spec = { input.resolve_spec(",
        ),
        "            )?;\n            let helper =",
        "            )? };\n            let helper =",
    );
    let closure_wrapped = replace_once(
        &replace_once(
            ACTUAL_SOURCE,
            "let fetch_spec = input.resolve_spec(",
            "let fetch_spec = (|| input.resolve_spec(",
        ),
        "            )?;\n            let helper =",
        "            ))()?;\n            let helper =",
    );
    let mut cases = owner_and_binding_mutations();
    cases.extend(resolution_call_mutations());
    cases.extend(projection_mutations());
    cases.extend(final_mutations(block_wrapped, closure_wrapped));
    for candidate in cases {
        rejected(&candidate)?;
    }
    Ok(())
}
