use super::intent;
use crate::reconcile::{before_advertise, occupies};
use crate::{IntentState, Reconcile};

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
