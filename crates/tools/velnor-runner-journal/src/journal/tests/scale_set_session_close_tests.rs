//! One-shot Scale Set close claims and schema upgrade coverage.

use crate::journal::{
    ReplayRoute, ScaleSetSessionClaim, ScaleSetSessionCloseClaim, ScaleSetSessionIdentity,
};
use crate::{DrainSnapshot, Journal};
use velnor_runner_github::RepositorySessionCloseClaim;

use super::Scratch;

fn identity(set_id: i64) -> Result<ScaleSetSessionIdentity, crate::HostError> {
    ScaleSetSessionIdentity::new(
        ReplayRoute {
            destination: "https://api.github.com",
            registration_scope: "repository",
            owner: "acme",
            repository: "widget",
            runner_group_id: 2,
            runner_group_name: "trusted",
            scale_set_id: set_id,
            scale_set_name: "linux",
        },
        829_618_808,
        "acme/widget",
    )
}

#[tokio::test]
async fn organization_session_is_not_claimed_by_repository_cleanup_route() -> Result<(), String> {
    let scratch = Scratch::new("session-org-close").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let route = ScaleSetSessionIdentity::new(
        ReplayRoute {
            destination: "https://api.github.com",
            registration_scope: "organization",
            owner: "acme",
            repository: "",
            runner_group_id: 2,
            runner_group_name: "trusted",
            scale_set_id: 3,
            scale_set_name: "linux",
        },
        829_618_808,
        "acme/widget",
    )
    .map_err(|error| error.to_string())?;
    let ScaleSetSessionClaim::Reserved(id) = journal
        .reserve_scale_set_session_if_accepting(&route)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("organization session reservation was not created".to_owned());
    };
    journal
        .record_scale_set_session_created(id, "org-session-1")
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        journal
            .claim_scale_set_session_close(&route)
            .await
            .map_err(|error| error.to_string())?,
        ScaleSetSessionCloseClaim::Held
    );
    Ok(())
}

#[tokio::test]
async fn close_claim_is_one_shot_across_reopen() -> Result<(), String> {
    let scratch = Scratch::new("session-close-reopen").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let route = identity(3).map_err(|error| error.to_string())?;
    let intent_id = {
        let journal = Journal::open(&path)
            .await
            .map_err(|error| error.to_string())?;
        let ScaleSetSessionClaim::Reserved(id) = journal
            .reserve_scale_set_session_if_accepting(&route)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Err("session reservation was not created".to_owned());
        };
        journal
            .record_scale_set_session_created(id, "session-close-once")
            .await
            .map_err(|error| error.to_string())?;
        let ScaleSetSessionCloseClaim::Claimed(permit) = journal
            .claim_scale_set_session_close(&route)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Err("open session did not yield a close permit".to_owned());
        };
        assert_eq!(permit.intent_id(), id);
        assert_eq!(permit.destination(), "https://api.github.com");
        assert_eq!(permit.registration_scope(), "repository");
        assert_eq!(permit.scope_name(), "acme/widget");
        assert_eq!(permit.target_repository_id(), 829_618_808);
        assert_eq!(permit.target_repository_full_name(), "acme/widget");
        assert_eq!(permit.runner_group_id(), 2);
        assert_eq!(permit.runner_group_name(), "trusted");
        assert_eq!(permit.scale_set_id(), 3);
        assert_eq!(permit.scale_set_name(), "linux");
        assert_eq!(permit.session_id_for_cleanup(), "session-close-once");
        assert!(
            journal
                .record_scale_set_session_closed(&permit)
                .await
                .is_err(),
            "a close receipt cannot be recorded before DELETE dispatch"
        );
        let mut permit = permit;
        assert!(permit.begin_delete_attempt());
        assert!(!permit.begin_delete_attempt());
        id
    };
    let reopened = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        reopened
            .claim_scale_set_session_close(&route)
            .await
            .map_err(|error| error.to_string())?,
        ScaleSetSessionCloseClaim::Held,
        "a crash after claiming close must never replay DELETE"
    );
    assert_eq!(
        reopened
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?,
        DrainSnapshot {
            draining: false,
            occupied_launches: 0,
            unresolved_intents: 1,
        }
    );
    assert!(intent_id > 0);
    Ok(())
}

#[tokio::test]
async fn v9_open_session_migrates_and_remains_close_claimable() -> Result<(), String> {
    let scratch = Scratch::new("session-v9-migration").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let route = identity(3).map_err(|error| error.to_string())?;
    let intent_id = {
        let journal = Journal::open(&path)
            .await
            .map_err(|error| error.to_string())?;
        let ScaleSetSessionClaim::Reserved(id) = journal
            .reserve_scale_set_session_if_accepting(&route)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Err("session reservation was not created".to_owned());
        };
        journal
            .record_scale_set_session_created(id, "session-from-v9")
            .await
            .map_err(|error| error.to_string())?;
        id
    };
    install_v9_session_table(&path, intent_id, "session-from-v9").await?;
    let migrated = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        migrated
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?
            .unresolved_intents,
        1,
        "migration must preserve the live session reservation"
    );
    let ScaleSetSessionCloseClaim::Claimed(permit) = migrated
        .claim_scale_set_session_close(&route)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("migrated repository session lost its exact route".to_owned());
    };
    assert_eq!(permit.intent_id(), intent_id);
    assert_eq!(permit.session_id_for_cleanup(), "session-from-v9");
    let mut permit = permit;
    assert!(permit.begin_delete_attempt());
    migrated
        .record_scale_set_session_closed(&permit)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn v8_current_published_schema_upgrades_to_v11() -> Result<(), String> {
    let scratch = Scratch::new("session-v8-migration").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    let (launch_id, _) = journal
        .begin_launch("v8-unresolved-launch")
        .await
        .map_err(|error| error.to_string())?;
    journal
        .record_launch_effect_intent(launch_id)
        .await
        .map_err(|error| error.to_string())?;
    journal
        .request_drain()
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE scale_set_sessions", ())
        .await
        .map_err(|error| error.to_string())?;
    conn.execute("PRAGMA user_version = 8", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(conn);
    drop(database);
    let upgraded = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(
        upgraded
            .drain_snapshot()
            .await
            .map_err(|error| error.to_string())?,
        DrainSnapshot {
            draining: true,
            occupied_launches: 1,
            unresolved_intents: 0,
        }
    );
    Ok(())
}

#[tokio::test]
async fn unsupported_newer_schema_is_not_rewritten_or_downgraded() -> Result<(), String> {
    let scratch = Scratch::new("session-future-schema").map_err(|error| error.to_string())?;
    let path = scratch.file();
    let journal = Journal::open(&path)
        .await
        .map_err(|error| error.to_string())?;
    drop(journal);
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("PRAGMA user_version = 12", ())
        .await
        .map_err(|error| error.to_string())?;
    drop(conn);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    let mut rows = conn
        .query("PRAGMA user_version", ())
        .await
        .map_err(|error| error.to_string())?;
    let version = rows
        .next()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "user_version returned no row".to_owned())?
        .get::<i64>(0)
        .map_err(|error| error.to_string())?;
    assert_eq!(version, 12, "opening a newer schema must not rewind it");
    Ok(())
}

async fn install_v9_session_table(
    path: &std::path::Path,
    intent_id: i64,
    session_id: &str,
) -> Result<(), String> {
    let database = turso::Builder::new_local(
        path.to_str()
            .ok_or_else(|| "journal path was not UTF-8".to_owned())?,
    )
    .build()
    .await
    .map_err(|error| error.to_string())?;
    let conn = database.connect().map_err(|error| error.to_string())?;
    conn.execute("DROP TABLE scale_set_sessions", ())
        .await
        .map_err(|error| error.to_string())?;
    conn.execute(
        "CREATE TABLE scale_set_sessions (intent_id INTEGER PRIMARY KEY CHECK (intent_id > 0), session_id TEXT UNIQUE, state TEXT NOT NULL CHECK (state IN ('creating', 'open', 'closed')))",
        (),
    )
    .await
    .map_err(|error| error.to_string())?;
    conn.execute(
        "INSERT INTO scale_set_sessions (intent_id, session_id, state) VALUES (?1, ?2, 'open')",
        (intent_id, session_id),
    )
    .await
    .map_err(|error| error.to_string())?;
    conn.execute("PRAGMA user_version = 9", ())
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn concurrent_close_claims_yield_exactly_one_delete_permit() -> Result<(), String> {
    let scratch = Scratch::new("session-close-race").map_err(|error| error.to_string())?;
    let journal = Journal::open(&scratch.file())
        .await
        .map_err(|error| error.to_string())?;
    let route = identity(3).map_err(|error| error.to_string())?;
    let ScaleSetSessionClaim::Reserved(id) = journal
        .reserve_scale_set_session_if_accepting(&route)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Err("session reservation was not created".to_owned());
    };
    journal
        .record_scale_set_session_created(id, "session-close-race")
        .await
        .map_err(|error| error.to_string())?;

    let (left, right) = tokio::join!(
        journal.claim_scale_set_session_close(&route),
        journal.claim_scale_set_session_close(&route),
    );
    let left = left.map_err(|error| error.to_string())?;
    let right = right.map_err(|error| error.to_string())?;
    assert_ne!(
        matches!(left, ScaleSetSessionCloseClaim::Claimed(_)),
        matches!(right, ScaleSetSessionCloseClaim::Claimed(_)),
        "only one concurrent caller can receive the durable delete permit"
    );
    assert!(matches!(
        (left, right),
        (
            ScaleSetSessionCloseClaim::Claimed(_),
            ScaleSetSessionCloseClaim::Held
        ) | (
            ScaleSetSessionCloseClaim::Held,
            ScaleSetSessionCloseClaim::Claimed(_)
        )
    ));
    Ok(())
}
