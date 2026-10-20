use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use super::super::state::PreparedConnection;

pub(super) struct ControllerState(PathBuf);

impl ControllerState {
    pub(super) fn new(binding: &crate::config::RepositoryConnection) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("velnor-successor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        let connection = toml::to_string(binding)
            .map_err(|error| error.to_string())?
            .replace("[scale_set]", "[connections.scale_set]")
            .replace("[source_policy]", "[connections.source_policy]");
        let text = format!(
            "schema = 2\n[[connections]]\n{connection}\n[host]\nmax_jobs = 1\n[host.resources]\nrunner_cpu_millicores = 1000\nrunner_memory_bytes = 4294967296\ndind_cpu_millicores = 1000\ndind_memory_bytes = 4294967296\noperational_memory_reserve_bytes = 1\n[docker]\ncontext = \"orbstack\"\nplatform = \"linux/amd64\"\nendpoint = \"unix:///var/run/docker.sock\"\n"
        );
        std::fs::write(path.join("host.toml"), text).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ControllerState {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) async fn prepared_connection(
    failure: &crate::launch::RunnerCreateFailure,
    journal: &crate::journal::Journal,
    state: &Path,
    binding: &crate::config::RepositoryConnection,
) -> Result<
    (
        PreparedConnection,
        crate::listen::Link,
        velnor_runner_github::QueueSession,
    ),
    String,
> {
    let bound = bind_session(failure, journal).await?;
    let (link, session) = create_session(bound.scale_set_id)?;
    let prepared = prepared_state(failure, state, binding, bound.scale_set_id, bound.intent_id)?;
    Ok((prepared, link, session))
}

struct BoundSession {
    scale_set_id: i64,
    intent_id: i64,
}

async fn bind_session(
    failure: &crate::launch::RunnerCreateFailure,
    journal: &crate::journal::Journal,
) -> Result<BoundSession, String> {
    journal
        .bootstrap_header_authority()
        .await
        .map_err(|error| error.to_string())?;
    let scope = failure.scope();
    let pending = scope
        .prepare_session_create("fedcba9876543210fedcba9876543210")
        .await
        .map_err(|error| error.to_string())?;
    let claim = scope
        .claim_session_create(pending.local_nonce())
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The controller test session claim is absent.".to_owned())?;
    let bound = scope
        .record_created_session(&claim, "successor-controller-session", 1)
        .await
        .map_err(|error| error.to_string())?;
    let set_id = scope
        .scale_set()
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "The controller test Scale Set is absent.".to_owned())?
        .id;
    Ok(BoundSession {
        scale_set_id: set_id,
        intent_id: bound.intent_id(),
    })
}

fn create_session(
    set_id: i64,
) -> Result<(crate::listen::Link, velnor_runner_github::QueueSession), String> {
    let link = crate::listen::inert_link_for_offline_test();
    let mut response = SessionResponse;
    let session =
        velnor_runner_github::create_session(&mut response, set_id, "owner", "admin-token")
            .map_err(|error| error.to_string())?;
    Ok((link, session))
}

fn prepared_state(
    failure: &crate::launch::RunnerCreateFailure,
    state: &Path,
    binding: &crate::config::RepositoryConnection,
    set_id: i64,
    intent_id: i64,
) -> Result<PreparedConnection, String> {
    let guest_image =
        crate::worker::guest_image::write_guest_image(state, binding, failure.images())
            .map_err(|error| error.to_string())?;
    Ok(PreparedConnection {
        binding: binding.clone(),
        scope: failure.scope().clone(),
        set_id,
        session_intent: intent_id,
        assigned_total: None,
        poll_cursor: 0,
        poll_retry_at: None,
        guest_image,
        completion: None,
        credentials: None,
        runner_ready: crate::launch::scoped_runtime::RunnerReadyRegistry::new(),
        ready_intents: BTreeMap::default(),
        authorizing: BTreeMap::default(),
        pending_bootstrap: BTreeMap::default(),
        unkeyed_bootstrap: Vec::new(),
        authorized: BTreeMap::default(),
        pending_authorization: BTreeMap::default(),
        unkeyed_authorization: Vec::new(),
        pending_acquired_recovery: BTreeMap::default(),
        unkeyed_acquired_recovery: Vec::new(),
    })
}

pub(super) fn controller_config(
    binding: &crate::config::RepositoryConnection,
) -> Result<crate::config::HostConfig, String> {
    let connection = toml::to_string(binding)
        .map_err(|error| error.to_string())?
        .replace("[scale_set]", "[connections.scale_set]")
        .replace("[source_policy]", "[connections.source_policy]");
    let mut config = crate::config::HostConfig::parse(&format!(
        "schema = 2\n[[connections]]\n{connection}\n[host]\nmax_jobs = 1\n[host.resources]\nrunner_cpu_millicores = 1000\nrunner_memory_bytes = 4294967296\ndind_cpu_millicores = 1000\ndind_memory_bytes = 4294967296\noperational_memory_reserve_bytes = 1\n[docker]\ncontext = \"orbstack\"\nplatform = \"linux/amd64\"\nendpoint = \"unix:///var/run/docker.sock\"\n"
    ))
    .map_err(|error| error.to_string())?;
    config
        .add_test_job_authority_policy_generation(&binding.connection_id, 1)
        .map_err(|error| error.to_string())?;
    Ok(config)
}

pub(super) fn assigned_statistics(total: i64) -> velnor_runner_github::Poll {
    velnor_runner_github::Poll::Batch(velnor_runner_github::ParsedBatch {
        message_id: 90_001,
        raw_body: String::new(),
        statistics: Some(velnor_runner_github::Statistics {
            total_available_jobs: 0,
            total_acquired_jobs: 1,
            total_assigned_jobs: total,
            total_running_jobs: 0,
            total_registered_runners: 0,
            total_busy_runners: 0,
            total_idle_runners: 0,
        }),
        jobs: Vec::new(),
    })
}

pub(super) async fn header_allocation(
    scope: &crate::journal::ConnectionJournal,
    session_intent: i64,
) -> Result<crate::journal::HeaderAllocation, String> {
    scope
        .session_header_allocation(session_intent)
        .await
        .map_err(|error| error.to_string())
}

struct SessionResponse;

impl velnor_runner_github::Transport for SessionResponse {
    fn exchange(
        &mut self,
        request: &velnor_runner_github::SessionRequest,
    ) -> Result<velnor_runner_github::Exchange, velnor_runner_github::TransportFail> {
        if request.method != velnor_runner_github::Method::Post {
            return Err(velnor_runner_github::TransportFail::Reset);
        }
        Ok(velnor_runner_github::Exchange {
            status: 200,
            body: br#"{"sessionId":"successor-controller-session","messageQueueUrl":"https://queue.example.invalid/messages","messageQueueAccessToken":"queue-token"}"#.to_vec(),
        })
    }
}
