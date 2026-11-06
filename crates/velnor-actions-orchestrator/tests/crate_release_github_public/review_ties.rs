// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn later_review_decision_supersedes_older_same_second_ambiguity() {
    let mut fixture = Fixture::valid();
    let approved = fixture.reviews[0].clone();
    let mut changes = approved.clone();
    changes["id"] = json!(2);
    changes["state"] = json!("CHANGES_REQUESTED");
    let mut later = approved.clone();
    later["id"] = json!(3);
    later["submitted_at"] = json!("2026-11-03T15:00:01Z");
    for reviews in [
        vec![approved.clone(), changes.clone(), later.clone()],
        vec![later.clone(), changes.clone(), approved.clone()],
        vec![changes.clone(), later.clone(), approved.clone()],
    ] {
        fixture.reviews = reviews;
        assert!(verify_github_evidence(&request(), &fixture).is_ok());
    }
    fixture.reviews = vec![approved.clone(), changes.clone()];
    assert!(verify_github_evidence(&request(), &fixture).is_err());
    later["state"] = json!("CHANGES_REQUESTED");
    fixture.reviews = vec![approved.clone(), changes.clone(), later.clone()];
    assert!(verify_github_evidence(&request(), &fixture).is_err());
    later["state"] = json!("APPROVED");
    later["user"]["login"] = json!("another-reviewer");
    later["user"]["id"] = json!(56);
    fixture.reviews = vec![approved.clone(), changes.clone(), later.clone()];
    assert!(verify_github_evidence(&request(), &fixture).is_err());
    changes["id"] = approved["id"].clone();
    later["user"] = approved["user"].clone();
    fixture.reviews = vec![approved, changes, later];
    assert!(verify_github_evidence(&request(), &fixture).is_err());
}
