use super::fixture::{BoundFixture, bound_fixture};
use crate::journal::Journal;

#[tokio::test]
async fn bound_journal_snapshot_reopens_with_thirteen_roles_and_exact_aliases() {
    let fixture = bound_fixture('b').await.expect("bound fixture");
    let revision_before_snapshot = fixture
        .journal
        .revision()
        .await
        .expect("revision before read-only snapshot");
    let first = fixture
        .scope
        .job_facts_snapshot(
            &fixture.claim,
            &fixture.ready.ready,
            &fixture.channel,
            &"a".repeat(64),
        )
        .await
        .expect("snapshot from bound journal rows");
    assert_eq!(
        fixture
            .journal
            .revision()
            .await
            .expect("revision after read-only snapshot"),
        revision_before_snapshot,
        "snapshot reads must not change the reservation journal"
    );
    let first_bytes = first.bytes().to_vec();
    assert_eq!(
        first.physical_binding_digest(),
        fixture.claim.instance_binding_digest()
    );
    let value: serde_json::Value =
        serde_json::from_slice(&first_bytes).expect("consumer document JSON");
    assert_guest_image_and_cache_aliases(&value);

    let BoundFixture {
        scratch,
        repository,
        journal,
        scope,
        ready,
        channel,
        claim,
    } = fixture;
    drop(scope);
    drop(journal);
    let reopened = Journal::open(&scratch.file())
        .await
        .expect("reopen journal");
    let reopened_scope = reopened
        .connection_scope(&repository)
        .await
        .expect("reopen connection scope");
    let after_reopen = reopened_scope
        .job_facts_snapshot(&claim, &ready.ready, &channel, &"a".repeat(64))
        .await
        .expect("same durable facts after reopen");
    assert_eq!(after_reopen.bytes(), first_bytes);
}

fn assert_guest_image_and_cache_aliases(value: &serde_json::Value) {
    assert_eq!(
        value["guest_image_sha256"],
        serde_json::json!("a".repeat(64))
    );
    let roles = value["instance_binding"]["roles"]
        .as_array()
        .expect("roles array");
    assert_eq!(roles.len(), 13);
    for role in ["cache-live", "cache-stage"] {
        assert_eq!(roles.iter().filter(|item| item["role"] == role).count(), 1);
        let alias = roles
            .iter()
            .find(|item| item["role"] == role)
            .expect("cache alias role exists");
        let work = roles
            .iter()
            .find(|item| item["role"] == "runner-work")
            .expect("Runner Work role exists");
        assert_eq!(alias["pool_id"], work["pool_id"]);
        assert_eq!(
            alias["filesystem_instance_digest"],
            work["filesystem_instance_digest"]
        );
    }
    assert_eq!(value["cache"]["live_path"], "/__w/_temp/velnor/cache");
    assert_eq!(
        value["cache"]["stage_path"],
        "/__w/_temp/velnor/cache-staging"
    );
}
