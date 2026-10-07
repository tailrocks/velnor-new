use crate::{DeleteDecision, delete_decision};

#[test]
fn names_are_not_delete_authority() {
    let owned = "sha256:owned";
    assert_eq!(delete_decision(owned, Some(owned)), DeleteDecision::Delete);
    assert_eq!(
        delete_decision(owned, Some("velnor-runner")),
        DeleteDecision::KeepForeign
    );
    assert_eq!(
        delete_decision(owned, Some("sha256:other")),
        DeleteDecision::KeepForeign
    );
    assert_eq!(delete_decision(owned, None), DeleteDecision::NotDeleted);
    assert_eq!(delete_decision("", Some("")), DeleteDecision::KeepForeign);
    assert_eq!(delete_decision("", None), DeleteDecision::NotDeleted);
}

#[test]
fn cleanup_set_keeps_foreign_name_collision() {
    let owned_id = "sha256:owned";
    let foreign_id = "sha256:foreign";
    let unrelated_id = "sha256:unrelated";
    let observed = [
        ("velnor-runner", owned_id),
        ("velnor-runner", foreign_id),
        ("other", unrelated_id),
    ];
    let mut delete_ids = Vec::new();
    let mut kept = Vec::new();
    for (name, id) in observed {
        let decision = delete_decision(owned_id, Some(id));
        if decision == DeleteDecision::Delete {
            delete_ids.push((name, id));
        }
        if decision == DeleteDecision::KeepForeign {
            kept.push((name, id));
        }
    }
    assert_eq!(delete_ids, vec![("velnor-runner", owned_id)]);
    assert_eq!(
        kept,
        vec![("velnor-runner", foreign_id), ("other", unrelated_id),]
    );
}
