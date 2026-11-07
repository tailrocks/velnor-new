// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn release_requires_valid_human_reviews_and_protected_environment() {
    let mut bad = Fixture::valid();
    bad.reviews[0]["user"]["type"] = json!("Bot");
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "bot cannot approve a release"
    );
    let mut bad = Fixture::valid();
    bad.reviews[0]["author_association"] = json!("NONE");
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "untrusted reviewer cannot approve a release"
    );
    let mut bad = Fixture::valid();
    bad.reviewer_permission = "read";
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "reviewer without write permission cannot authorize release"
    );
    let mut bad = Fixture::valid();
    bad.reviews[0]["state"] = json!("DISMISSED");
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "dismissed approval cannot authorize release"
    );
    let mut bad = Fixture::valid();
    let mut later = bad.reviews[0].clone();
    later["id"] = json!(0);
    later["state"] = json!("CHANGES_REQUESTED");
    later["submitted_at"] = json!("2026-11-03T15:01:00Z");
    bad.reviews.push(later);
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "a later review must prevail even with a smaller ID"
    );
    let mut bad = Fixture::valid();
    let mut bot = bad.reviews[0].clone();
    bot["id"] = json!(3);
    bot["user"] = json!({"login":"security-bot","id":56,"type":"Bot"});
    bot["state"] = json!("CHANGES_REQUESTED");
    bad.reviews.push(bot);
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "outstanding bot changes request cannot be ignored"
    );
    let mut bad = Fixture::valid();
    bad.reviews.clear();
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "unreviewed release must not publish"
    );
    let mut bad = Fixture::valid();
    bad.reviews[0]["submitted_at"] = json!("2026-11-03T15:38:50Z");
    assert!(
        verify_github_evidence(&request(), &bad).is_err(),
        "approval in the API merge timestamp second is ambiguous"
    );
}

#[test]
fn release_review_decisions_use_authority_and_immutable_actor_identity() {
    for (login, id, association, kind) in [
        ("outsider", 58, "NONE", "User"),
        ("read-only", 57, "MEMBER", "User"),
        ("outside-bot", 59, "NONE", "Bot"),
    ] {
        let mut fixture = Fixture::valid();
        let mut review = fixture.reviews[0].clone();
        review["id"] = json!(2);
        review["user"] = json!({"login":login,"id":id,"type":kind});
        review["author_association"] = json!(association);
        review["state"] = json!("CHANGES_REQUESTED");
        review["submitted_at"] = json!("2026-11-03T15:01:00Z");
        fixture.reviews.push(review);
        assert!(
            verify_github_evidence(&request(), &fixture).is_ok(),
            "{login}"
        );
    }
    let mut renamed_author = Fixture::valid();
    renamed_author.author_id = 55;
    assert!(verify_github_evidence(&request(), &renamed_author).is_err());
}

#[test]
fn release_review_cannot_turn_an_authorized_bot_into_a_human() {
    let mut fixture = Fixture::valid();
    let mut bot = fixture.reviews[0].clone();
    bot["id"] = json!(2);
    bot["user"] = json!({"login":"security-bot","id":56,"type":"Bot"});
    bot["state"] = json!("CHANGES_REQUESTED");
    bot["submitted_at"] = json!("2026-11-03T15:01:00Z");
    let mut impersonation = bot.clone();
    impersonation["id"] = json!(3);
    impersonation["user"]["type"] = json!("User");
    impersonation["state"] = json!("APPROVED");
    impersonation["submitted_at"] = json!("2026-11-03T15:02:00Z");
    fixture.reviews.extend([bot, impersonation]);
    assert!(verify_github_evidence(&request(), &fixture).is_err());
}
