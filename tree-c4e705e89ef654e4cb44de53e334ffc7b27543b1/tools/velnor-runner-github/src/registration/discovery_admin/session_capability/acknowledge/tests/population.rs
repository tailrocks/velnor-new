use std::time::{Duration, SystemTime};

use crate::PopulationObservationSource;

use super::*;

const CREATED_WITH_STATS: &str = r#"{"sessionId":"session-1","messageQueueUrl":"https://queue.example/_apis/runtime/runnerscalesets/7/sessions/session-1/messages","messageQueueAccessToken":"queue-canary","statistics":{"totalAvailableJobs":0,"totalAcquiredJobs":2,"totalAssignedJobs":2,"totalRunningJobs":0,"totalRegisteredRunners":2,"totalBusyRunners":1,"totalIdleRunners":1}}"#;

#[test]
fn create_stats_survive_empty_as_durable_data_then_current_poll_supersedes()
-> Result<(), &'static str> {
    let mut script = Script::with_session_responses(
        CREATED_WITH_STATS,
        vec![Ok(exchange(202, "")), Ok(exchange(200, STARTED_WITH_STATS))],
    );
    let connection = admin_connection_once(
        &mut script,
        &AdminConnectionCall {
            config_url: "https://github.com/ChainArgos/java-monorepo",
            registration_token: "registration-canary",
        },
    )
    .map_err(|_| "admin connection")?;
    let binding = PoolBinding {
        registration_scope: PoolRegistrationScope::Repository {
            owner: "ChainArgos".to_owned(),
            repository: "java-monorepo".to_owned(),
        },
        repository_id: 829_618_808,
        repository_full_name: "ChainArgos/java-monorepo".to_owned(),
        scale_set_id: SESSION_ID,
        scale_set_name: "synthetic-session-test-set".to_owned(),
        actions_runner_group_id: 1,
        actions_runner_group_name: "Default".to_owned(),
        rest_runner_group_id: Some(1),
        runner_image_profile: None,
        runner_image: None,
        policy_digest: "policy-digest".to_owned(),
    };
    let mut capability = VerifiedPoolSessionAdmin {
        connection,
        binding,
        policy_digest: "policy-digest".to_owned(),
        session_creation_attempted: false,
        created_session_id: None,
        close_attempted: false,
        expires_at: SystemTime::now() + Duration::from_secs(300),
    };
    let mut session = capability
        .create_session(&mut script, "velnor-host")
        .map_err(|_| "create session")?;
    let created = session
        .population_observation()
        .ok_or("create-time statistics")?;
    assert_eq!(
        created.source(),
        PopulationObservationSource::SessionCreated
    );
    assert_eq!(created.message_id(), None);
    assert_eq!(created.statistics().total_assigned_jobs, 2);

    assert!(matches!(
        capability.poll_with_trust(&mut script, &mut session, 0, 2, &RefreshGate::new()),
        Ok(PollWithTrust::Empty)
    ));
    assert!(
        session.population_observation().is_none(),
        "an Empty poll does not reuse stale counts as a live permit"
    );

    let PollWithTrust::Batch(batch) = capability
        .poll_with_trust(&mut script, &mut session, 0, 2, &RefreshGate::new())
        .map_err(|_| "poll with newer statistics")?
    else {
        return Err("new statistics batch expected");
    };
    assert_eq!(batch.message_id(), 19);
    let current = session
        .population_observation()
        .ok_or("new current statistics")?;
    assert_eq!(current.source(), PopulationObservationSource::PollBatch);
    assert_eq!(current.message_id(), Some(19));
    assert_eq!(current.statistics().total_registered_runners, 4);
    assert_eq!(script.seen.len(), 4);
    Ok(())
}
