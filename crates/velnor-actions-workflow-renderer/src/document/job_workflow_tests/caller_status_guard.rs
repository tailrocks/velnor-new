use std::collections::BTreeSet;

use crate::{
    RenderError,
    lane_share::job_workflow_extract::{JobWorkflowSelection, PROTOTYPE_JOB_ID},
    yaml::Yaml,
};

#[test]
fn selected_caller_guards_default_bare_and_wrapped_conditions() -> Result<(), RenderError> {
    let cases = [
        (None, "!cancelled() && success()"),
        (
            Some("needs.plan.result == 'success'"),
            "!cancelled() && success() && (needs.plan.result == 'success')",
        ),
        (
            Some("${{ needs.plan.result == 'success' }}"),
            "${{ !cancelled() && success() && ( needs.plan.result == 'success' ) }}",
        ),
    ];

    for (condition, expected) in cases {
        assert_eq!(
            serialized_caller_if(condition, true)?,
            Some(expected.to_owned())
        );
    }
    Ok(())
}

#[test]
fn selected_caller_preserves_existing_status_functions() -> Result<(), RenderError> {
    let conditions = [
        "always() && needs.plan.result == 'success'",
        "${{ failure() || needs.plan.result == 'success' }}",
        "${{ cancelled() }}",
    ];

    for condition in conditions {
        assert_eq!(
            serialized_caller_if(Some(condition), true)?,
            Some(condition.to_owned())
        );
    }
    Ok(())
}

#[test]
fn quoted_status_function_names_do_not_disable_caller_guard() -> Result<(), RenderError> {
    let condition = "needs.plan.outputs.note == 'always()' && needs.plan.outputs.state == 'failure() cancelled()'";
    assert_eq!(
        serialized_caller_if(Some(condition), true)?,
        Some("!cancelled() && success() && (needs.plan.outputs.note == 'always()' && needs.plan.outputs.state == 'failure() cancelled()')".to_owned())
    );
    Ok(())
}

#[test]
fn success_only_caller_conditions_are_not_relocated() {
    for condition in ["success()", "success() || needs.plan.result == 'failure'"] {
        assert!(serialized_caller_if(Some(condition), true).is_err());
    }
}

#[test]
fn legacy_caller_conditions_remain_unchanged() -> Result<(), RenderError> {
    let condition = "${{ needs.plan.result == 'success' }}";
    assert_eq!(serialized_caller_if(None, false)?, None);
    assert_eq!(
        serialized_caller_if(Some(condition), false)?,
        Some(condition.to_owned())
    );
    Ok(())
}

fn serialized_caller_if(
    condition: Option<&str>,
    explicit_selection: bool,
) -> Result<Option<String>, RenderError> {
    let mut source = super::source_job();
    source.condition = condition.map(str::to_owned);
    let workflow = super::workflow(source.clone());
    let mut shared = super::lane_share(source);
    if explicit_selection {
        shared
            .job_workflows
            .get_mut(PROTOTYPE_JOB_ID)
            .expect("prototype source is prepared")
            .selection =
            JobWorkflowSelection::explicit(&BTreeSet::from([PROTOTYPE_JOB_ID.to_owned()]));
    }

    let parent = super::workflow_to_yaml(&workflow, &shared, &super::context(), &BTreeSet::new())?;
    let Yaml::Map(workflow_fields) = parent else {
        panic!("workflow serializes as a map");
    };
    let Some((_, Yaml::Map(jobs))) = workflow_fields.iter().find(|(key, _)| key == "jobs") else {
        panic!("workflow serializes its jobs map");
    };
    let Some((_, Yaml::Map(caller_fields))) = jobs.iter().find(|(key, _)| key == PROTOTYPE_JOB_ID)
    else {
        panic!("prototype caller is serialized");
    };

    match caller_fields.iter().find(|(key, _)| key == "if") {
        None => Ok(None),
        Some((_, Yaml::Str(condition))) => Ok(Some(condition.clone())),
        Some(_) => panic!("caller condition serializes as a string"),
    }
}
