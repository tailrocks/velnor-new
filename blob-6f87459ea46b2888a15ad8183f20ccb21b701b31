//! Journal reconcile gating: advertise holds, adoption, and occupancy.
//!
//! Declared via `#[path]` from `lib.rs` under `cfg(test)`; split from
//! `journal_tests.rs` by the 400-line repo-size gate.

use crate::reconcile::{before_advertise, occupies};
use crate::{IntentRow, IntentState, Reconcile};

fn intent(state: IntentState, kind: &str) -> IntentRow {
    IntentRow {
        id: 1,
        kind: kind.to_owned(),
        subject: "job".to_owned(),
        state,
        docker_id: None,
        dind_id: None,
        worker_volume: None,
        scale_set_id: None,
        request_id: None,
        runner_name: None,
        docker_engine_id: None,
        launch_phase: None,
        github_runner_id: None,
        cleanup_proven: false,
        launch_id: None,
        assignment_key: None,
        seed_generation_id: None,
        acquire_attempted: false,
        acquire_resolved: false,
        acquired: false,
        jit_requested: false,
        runner_completed: false,
    }
}

#[test]
fn before_advertise_holds_uncertain_and_adopts() {
    let pending = intent(IntentState::Pending, "acquire");
    let uncertain = intent(IntentState::Uncertain, "acquire");
    assert_eq!(
        before_advertise(&[pending], &[], &[], &[]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
    assert_eq!(
        before_advertise(&[uncertain], &[], &[], &[]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
    let mut missing = intent(IntentState::Done, "delete");
    missing.docker_id = Some("ctr-missing".to_owned());
    missing.github_runner_id = Some("gh-1".to_owned());
    assert_eq!(
        before_advertise(&[missing.clone()], &[], &["gh-1"], &["ctr-missing"]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
    missing.docker_id = Some("ctr-seen".to_owned());
    assert_eq!(
        before_advertise(&[missing], &["ctr-seen"], &[], &["ctr-seen"]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
    assert_eq!(
        before_advertise(&[], &["ctr-owned", "ctr-foreign"], &[], &["ctr-owned"]),
        Reconcile::Hold {
            adopt: vec!["ctr-owned".to_owned()],
            occupied: 0,
        }
    );
    let mut settled = intent(IntentState::Done, "delete");
    settled.docker_id = Some("ctr-seen".to_owned());
    settled.github_runner_id = Some("gh-1".to_owned());
    assert_eq!(
        before_advertise(
            &[settled.clone()],
            &["ctr-seen", "ctr-owned"],
            &["gh-1"],
            &["ctr-seen", "ctr-owned"],
        ),
        Reconcile::Hold {
            adopt: vec!["ctr-owned".to_owned()],
            occupied: 1,
        }
    );
    assert_eq!(
        before_advertise(&[settled], &["ctr-seen"], &["gh-1"], &["ctr-seen"]),
        Reconcile::Advertise { occupied: 1 }
    );
}

#[test]
fn failed_acquire_does_not_occupy_and_clean_rows_advertise() {
    let failed = intent(IntentState::Failed, "acquire");
    assert!(!occupies(&failed));
    assert_eq!(
        before_advertise(&[failed], &[], &[], &[]),
        Reconcile::Advertise { occupied: 0 }
    );
    let done = intent(IntentState::Done, "acquire");
    assert!(occupies(&done));
    assert_eq!(
        before_advertise(&[done], &[], &[], &[]),
        Reconcile::Advertise { occupied: 1 }
    );
    let mut proved = intent(IntentState::Done, "delete");
    proved.docker_id = Some("ctr-gone".to_owned());
    proved.cleanup_proven = true;
    assert!(!occupies(&proved));
    assert_eq!(
        before_advertise(&[proved], &[], &[], &[]),
        Reconcile::Advertise { occupied: 0 }
    );
    let mut failed_delete = intent(IntentState::Failed, "delete");
    failed_delete.docker_id = Some("ctr-still".to_owned());
    assert!(occupies(&failed_delete));
    assert_eq!(
        before_advertise(&[failed_delete], &[], &[], &["ctr-still"]),
        Reconcile::Hold {
            adopt: vec![],
            occupied: 1,
        }
    );
}
