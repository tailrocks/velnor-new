use super::super::{caller_condition_for_selection, pair_is_relocatable};
use super::source_job;
use crate::lane_share::job_workflow_extract::PROTOTYPE_JOB_ID;

#[test]
fn adds_explicit_default_success_to_conditions_without_status_functions() {
    assert_eq!(
        caller_condition_for_selection(PROTOTYPE_JOB_ID, None)
            .expect("a missing source condition gets the default status gate"),
        "!cancelled() && success()"
    );
    assert_eq!(
        caller_condition_for_selection(PROTOTYPE_JOB_ID, Some("needs.plan.result == 'success'"),)
            .expect("a bare source condition gets the default status gate"),
        "!cancelled() && success() && (needs.plan.result == 'success')"
    );
    assert_eq!(
        caller_condition_for_selection(
            PROTOTYPE_JOB_ID,
            Some("${{ needs.plan.result == 'success' }}"),
        )
        .expect("a wrapped source condition keeps one expression wrapper"),
        "${{ !cancelled() && success() && ( needs.plan.result == 'success' ) }}"
    );
}

#[test]
fn preserves_nondefault_status_function_conditions_exactly() {
    for condition in [
        "always() && needs.plan.result == 'success'",
        "failure() || needs.plan.result == 'failure'",
        "!cancelled() && needs.plan.result == 'success'",
        "success() || failure()",
    ] {
        assert_eq!(
            caller_condition_for_selection(PROTOTYPE_JOB_ID, Some(condition))
                .expect("a nondefault status condition remains supported"),
            condition
        );
    }
}

#[test]
fn leaves_success_only_condition_pairs_inline() {
    let mut hosted = source_job(Vec::new());
    hosted.condition = Some("success() || needs.plan.result == 'failure'".to_owned());
    let mut local = hosted.clone();
    local.runs_on = "scale-set:velnor+ubuntu-26.04-scale-set".to_owned();

    assert!(!pair_is_relocatable(
        PROTOTYPE_JOB_ID,
        &hosted,
        "rust-velnor-actions-cli__local",
        &local,
    ));
    assert!(
        caller_condition_for_selection(PROTOTYPE_JOB_ID, hosted.condition.as_deref(),).is_err()
    );
}

#[test]
fn status_names_in_literals_identifiers_and_properties_do_not_change_the_gate() {
    let condition =
        "contains('always()', 'success()') && not_failure() && github.cancelled() == false";
    assert_eq!(
        caller_condition_for_selection(PROTOTYPE_JOB_ID, Some(condition))
            .expect("only status function calls control implicit success"),
        format!("!cancelled() && success() && ({condition})")
    );
    assert_eq!(
        caller_condition_for_selection(
            PROTOTYPE_JOB_ID,
            Some("ALWAYS() && needs.plan.result == 'success'"),
        )
        .expect("status function names are case-insensitive"),
        "ALWAYS() && needs.plan.result == 'success'"
    );
}
