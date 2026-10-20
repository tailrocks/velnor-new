use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_runner_github::TransportFail;
use velnor_runner_github::session::{Exchange, Method, SessionRequest, Transport};

use super::super::super::cleanup_retry_delay_seconds;
use crate::error::HostError;
use crate::journal::{CleanupClaim, CompletedLaunch, Journal};
use crate::launch::completion::journal_ref::CompletionJournal;
use crate::listen::Secret;

const SCALE_SET_ID: i64 = 77;
const REQUEST_ID: i64 = 61;
const RUNNER_ID: i64 = 10_061;
const RUNNER_NAME: &str = "v61";
const EFFECT_LEASE_SECONDS: i64 = 90;

pub(super) fn runner_body(id: i64, name: &str, scale_set_id: i64) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "count": 1,
        "value": [{ "id": id, "name": name, "runnerScaleSetId": scale_set_id }]
    }))
    .expect("runner fixture serializes")
}

pub(super) fn empty_runners() -> Vec<u8> {
    br#"{"count":0,"value":[]}"#.to_vec()
}

pub(super) enum Response {
    Json(u16, Vec<u8>),
    Status(u16),
    Failure(TransportFail),
}

impl Response {
    pub(super) fn json(status: u16, body: Vec<u8>) -> Self {
        Self::Json(status, body)
    }

    pub(super) fn status(status: u16) -> Self {
        Self::Status(status)
    }
}

pub(super) struct ScriptedTransport {
    pub(super) responses: VecDeque<Response>,
    pub(super) requests: Vec<CapturedRequest>,
}

impl ScriptedTransport {
    pub(super) fn new(responses: impl IntoIterator<Item = Response>) -> Self {
        Self {
            responses: responses.into_iter().collect(),
            requests: Vec::new(),
        }
    }
}

impl Transport for ScriptedTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.requests.push(CapturedRequest {
            method: request.method,
            path: request.path.clone(),
            query: request.query.clone(),
        });
        match self.responses.pop_front().ok_or(TransportFail::Reset)? {
            Response::Json(status, body) => Ok(Exchange { status, body }),
            Response::Status(status) => Ok(Exchange {
                status,
                body: Vec::new(),
            }),
            Response::Failure(error) => Err(error),
        }
    }
}

pub(super) struct CapturedRequest {
    pub(super) method: Method,
    pub(super) path: String,
    pub(super) query: Option<String>,
}

pub(super) struct Fixture {
    pub(super) journal: Journal,
    pub(super) completion: CompletionJournal,
    pub(super) admin: Secret,
    _scratch: Scratch,
}

impl Fixture {
    pub(super) async fn new(label: &str) -> Result<Self, HostError> {
        let scratch = Scratch::new(label)?;
        let journal = Journal::open(scratch.file()).await?;
        let completion = CompletionJournal::root(journal.clone());
        let admin = Secret::new("test token");
        Ok(Self {
            journal,
            completion,
            admin,
            _scratch: scratch,
        })
    }

    pub(super) async fn claim(&self) -> Result<(CompletedLaunch, CleanupClaim), HostError> {
        let (id, _) = self
            .journal
            .begin_assigned_launch("m61r61", SCALE_SET_ID, REQUEST_ID, RUNNER_NAME)
            .await?;
        self.journal
            .record_runner_completed(SCALE_SET_ID, REQUEST_ID, RUNNER_ID, RUNNER_NAME)
            .await?
            .ok_or(HostError::Journal)?;
        let launch = self.due(id).await?;
        let claim = self
            .journal
            .claim_completion_cleanup(id, EFFECT_LEASE_SECONDS)
            .await?
            .ok_or(HostError::Journal)?;
        Ok((launch, claim))
    }

    pub(super) async fn due(&self, id: i64) -> Result<CompletedLaunch, HostError> {
        self.journal
            .due_completed_launches(
                unix_seconds()?.checked_add(200).ok_or(HostError::Journal)?,
                10,
            )
            .await?
            .into_iter()
            .find(|launch| launch.intent.id == id)
            .ok_or(HostError::Journal)
    }

    pub(super) async fn retry_claim(
        &self,
        id: i64,
        previous: CleanupClaim,
    ) -> Result<CleanupClaim, HostError> {
        let now = unix_seconds()?;
        let retry_after = now
            .checked_add(cleanup_retry_delay_seconds(previous.attempt))
            .ok_or(HostError::Journal)?;
        if !self
            .journal
            .retry_completion_cleanup_at(id, previous.generation, now, retry_after)
            .await?
        {
            return Err(HostError::Journal);
        }
        self.journal
            .claim_completion_cleanup_at(id, retry_after, EFFECT_LEASE_SECONDS)
            .await?
            .ok_or(HostError::Journal)
    }
}

pub(super) fn assert_request(
    request: &CapturedRequest,
    method: Method,
    path: &str,
    query: Option<&str>,
) {
    assert_eq!(request.method, method);
    assert_eq!(request.path, path);
    assert_eq!(request.query.as_deref(), query);
}

struct Scratch {
    path: PathBuf,
    directory: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "velnor-completion-runner-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).map_err(|_| HostError::Path)?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
        Ok(Self {
            path: directory.join("journal.db"),
            directory,
        })
    }

    fn file(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.directory) {
            let _kind = error.kind();
        }
    }
}

fn unix_seconds() -> Result<i64, HostError> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| HostError::Journal)?
            .as_secs(),
    )
    .map_err(|_| HostError::Journal)
}
