use super::fixture::{BoundFixture, bound_fixture};
use crate::journal::Journal;

#[tokio::test]
async fn bound_journal_snapshot_reopens_with_thirteen_roles_and_exact_aliases() {
    let fixture = bound_fixture('b').await.expect("bound fixture");
    let first = fixture
        .scope
        .job_facts_snapshot(&fixture.claim, &fixture.ready.ready, &fixture.channel)
        .await
        .expect("snapshot from bound journal rows");
    let first_bytes = first.bytes().to_vec();
    assert_eq!(
        first.physical_binding_digest(),
        fixture.claim.instance_binding_digest()
    );
    let value: serde_json::Value =
        serde_json::from_slice(&first_bytes).expect("consumer document JSON");
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
        .job_facts_snapshot(&claim, &ready.ready, &channel)
        .await
        .expect("same durable facts after reopen");
    assert_eq!(after_reopen.bytes(), first_bytes);
}
