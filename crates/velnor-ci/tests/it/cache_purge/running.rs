use super::*;

#[test]
fn in_progress_runs_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("in_progress", 2), ("queued", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "3 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn queued_runs_alone_refuse_the_purge() {
    let api = Api::with_runs(mixed(), &[("queued", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn waiting_runs_alone_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("waiting", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn a_retained_action_required_run_does_not_block_the_purge() {
    let api = Api::with_runs(mixed(), &[("action_required", 1)]);
    let report = purge_private(&api, &repo(), true);
    assert!(report.is_ok(), "{report:?}");
    assert_eq!(api.deleted.borrow().len(), 3);
}

#[test]
fn enabled_workflows_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_workflows(mixed(), &["active", "disabled_manually"]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflows are enabled; disable every workflow before the purge so no run can start during it"
        )
    );
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn disabled_workflows_let_the_purge_proceed() {
    let api = Api::with_workflows(mixed(), &["disabled_manually", "disabled_inactivity"]);
    assert!(purge_private(&api, &repo(), true).is_ok());
}
#[test]
fn requested_runs_alone_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("requested", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn pending_runs_alone_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("pending", 1)]);
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn a_dry_run_warns_about_running_workflows_and_succeeds() {
    let api = Api::with_runs(mixed(), &[("in_progress", 2)]);
    let report = purge_private(&api, &repo(), false).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("would delete 3 entries"), "{report}");
    assert!(
        report.contains("2 workflow runs are in progress; wait for them or disable workflows"),
        "{report}"
    );
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn a_run_that_starts_after_the_in_progress_query_is_refused_on_the_second_pass() {
    let mut api = Api::with_runs(mixed(), &[("queued", 1)]);
    api.promoted_to = Some("in_progress");
    let error = purge_private(&api, &repo(), true)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        error.as_deref(),
        Some(
            "1 workflow runs are in progress; wait for them or disable workflows, then run purge-private again"
        )
    );
    assert!(api.deleted.borrow().is_empty());
    assert_eq!(api.entries.borrow().len(), 8);
}

#[test]
fn no_running_workflows_in_either_pass_lets_the_purge_proceed() {
    let api = Api::new(mixed());
    let report = purge_private(&api, &repo(), true).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(report.contains("deleted 3 entries"), "{report}");
    assert_eq!(api.run_listings.get(), 10);
}
