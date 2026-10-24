use super::confirm_locator_absence;
use crate::package_release_publish::PublicRepositorySnapshot;
use std::time::{Duration, Instant};
use velnor_actions_mise::CheckDeadline;

fn repository(branch: &str) -> PublicRepositorySnapshot {
    PublicRepositorySnapshot {
        id: 42,
        full_name: "example/project".to_owned(),
        default_branch: branch.to_owned(),
    }
}

#[test]
fn absence_is_proven_for_an_unchanged_safe_default_branch_snapshot() {
    let before = repository("trunk");
    assert!(
        confirm_locator_absence(
            "example/project",
            &before,
            &"a".repeat(40),
            None,
            &before,
            &"a".repeat(40),
        )
        .expect("stable absence")
    );
}

#[test]
fn present_locator_requires_full_authority_admission() {
    let snapshot = repository("main");
    assert!(
        !confirm_locator_absence(
            "example/project",
            &snapshot,
            &"a".repeat(40),
            Some(b"locator"),
            &snapshot,
            &"a".repeat(40),
        )
        .expect("present locator")
    );
}

#[test]
fn repository_identity_branch_and_tip_changes_reject_absence() {
    let before = repository("main");
    let mut changed_identity = before.clone();
    changed_identity.full_name = "different/project".to_owned();
    let mut changed_id = before.clone();
    changed_id.id = 43;
    let cases = [
        (changed_identity, "a".repeat(40)),
        (changed_id, "a".repeat(40)),
        (repository("trunk"), "a".repeat(40)),
        (before.clone(), "b".repeat(40)),
    ];
    for (after, after_tip) in cases {
        assert!(
            confirm_locator_absence(
                "example/project",
                &before,
                &"a".repeat(40),
                None,
                &after,
                &after_tip,
            )
            .is_err()
        );
    }
    assert!(
        confirm_locator_absence(
            "different/project",
            &before,
            &"a".repeat(40),
            None,
            &before,
            &"a".repeat(40),
        )
        .is_err()
    );
}

#[test]
fn invalid_repository_snapshot_fails_closed() {
    let mut snapshot = repository("main");
    snapshot.default_branch.clear();
    assert!(
        confirm_locator_absence(
            "example/project",
            &snapshot,
            &"a".repeat(40),
            None,
            &snapshot,
            &"a".repeat(40),
        )
        .is_err()
    );
}

#[test]
fn unavailable_repository_reads_fail_closed_on_expired_deadline() {
    let start = Instant::now()
        .checked_sub(Duration::from_secs(2))
        .expect("monotonic clock supports an expired test deadline");
    let deadline =
        CheckDeadline::from_start(start, Duration::from_secs(1)).expect("expired deadline shape");
    assert!(
        crate::homebrew::authority::protected_locator_absent_on_default_branch(
            "example/project",
            deadline,
        )
        .is_err()
    );
}
