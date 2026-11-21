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
fn action_required_runs_alone_refuse_the_purge_and_delete_nothing() {
    let api = Api::with_runs(mixed(), &[("action_required", 1)]);
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
