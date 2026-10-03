//! Immutable delivery evidence. Missing evidence never proves measured zero.

use crate::dispatch_admission::{AdmissionEntry, AdmissionKind, TerminalOutcome};
use crate::session::completed_report::{
    CORRELATION_ID_ENV, PARENT_SESSION_ID_ENV, ROOT_SESSION_ID_ENV, SESSION_ID_ENV,
    prepare_directory, valid_uuid, validate_correlation,
};
use eyre::{Result, bail};
use mbx_cache_core::{AdapterKind, MeasurementEvent};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DeliveryIdentity {
    pub session_id: String,
    pub root_session_id: String,
    pub parent_session_id: Option<String>,
    pub caller_correlation: Option<String>,
}

impl DeliveryIdentity {
    fn validate(&self) -> Result<()> {
        if !valid_uuid(&self.session_id) || !valid_uuid(&self.root_session_id) {
            bail!("invalid measurement delivery session identity");
        }
        match self.parent_session_id.as_deref() {
            Some(parent)
                if valid_uuid(parent)
                    && parent != self.session_id
                    && self.session_id != self.root_session_id => {}
            None if self.session_id == self.root_session_id => {}
            _ => bail!("invalid measurement delivery parent identity"),
        }
        validate_correlation(self.caller_correlation.as_deref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EventKind {
    Invocation,
    Process,
    Output,
}

#[derive(Debug)]
pub(crate) struct AdmissionClosed;

impl std::fmt::Display for AdmissionClosed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("measurement admission is closed")
    }
}

impl std::error::Error for AdmissionClosed {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeliveryFailure {
    NoSessionSocket,
    RequestFailed,
    UnexpectedAcknowledgement,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Stage<'a> {
    Attempted,
    Acknowledged {
        event: &'a MeasurementEvent,
        event_sha256: String,
    },
    Failed {
        reason: DeliveryFailure,
    },
}

#[derive(Debug, Serialize)]
struct Evidence<'a> {
    schema_version: u8,
    mbx_version: &'static str,
    source_base_version: &'static str,
    identity: &'a DeliveryIdentity,
    event_id: &'a str,
    adapter: AdapterKind,
    event_kind: EventKind,
    delivery: Stage<'a>,
}

/// One attempted event. Receipts never replace an attempt or each other.
/// A missing receipt or a failed receipt makes delivery unverified.
pub(crate) struct Delivery {
    directory: PathBuf,
    identity: DeliveryIdentity,
    event_id: String,
    adapter: AdapterKind,
    event_kind: EventKind,
    admission: Option<AdmissionEntry>,
}

impl Delivery {
    /// No configured report directory means no telemetry filesystem work.
    /// Invalid or incomplete configuration returns an error, never a zero.
    pub(crate) fn enroll(adapter: AdapterKind, event_kind: EventKind) -> Result<Option<Self>> {
        let Some(directory) = std::env::var_os("MBX_STATS_REPORT_DIR") else {
            return Ok(None);
        };
        let identity = DeliveryIdentity {
            session_id: required_environment(SESSION_ID_ENV)?,
            root_session_id: required_environment(ROOT_SESSION_ID_ENV)?,
            parent_session_id: optional_environment(PARENT_SESSION_ID_ENV)?,
            caller_correlation: optional_environment(CORRELATION_ID_ENV)?,
        };
        Self::enroll_at(PathBuf::from(directory), identity, adapter, event_kind).map(Some)
    }

    pub(crate) fn enroll_at(
        directory: PathBuf,
        identity: DeliveryIdentity,
        adapter: AdapterKind,
        event_kind: EventKind,
    ) -> Result<Self> {
        if !cfg!(unix) {
            bail!("private measurement delivery evidence is unsupported on this platform");
        }
        identity.validate()?;
        prepare_directory(&directory)?;
        let event_id = hex::encode(rand::random::<[u8; 16]>());
        let admission = if cfg!(feature = "owned-cache-transport") {
            let kind = match event_kind {
                EventKind::Invocation => AdmissionKind::Invocation,
                EventKind::Process => AdmissionKind::Process,
                EventKind::Output => AdmissionKind::Output,
            };
            Some(
                AdmissionEntry::enroll(
                    &directory,
                    &identity.session_id,
                    &identity.root_session_id,
                    &event_id,
                    adapter,
                    kind,
                )?
                .ok_or(AdmissionClosed)?,
            )
        } else {
            None
        };
        let delivery = Self {
            directory,
            identity,
            event_id,
            adapter,
            event_kind,
            admission,
        };
        delivery.publish(Stage::Attempted)?;
        Ok(delivery)
    }

    /// Bind the exact immutable event that the owning agent acknowledged.
    pub(crate) fn acknowledge(mut self, event: &MeasurementEvent) -> Result<PathBuf> {
        let (adapter, kind) = match event {
            MeasurementEvent::Invocation { adapter, .. } => (*adapter, EventKind::Invocation),
            MeasurementEvent::Process { adapter, .. } => (*adapter, EventKind::Process),
            MeasurementEvent::Output { adapter, .. } => (*adapter, EventKind::Output),
        };
        if adapter != self.adapter || kind != self.event_kind {
            bail!("measurement receipt event does not match enrollment");
        }
        let event_sha256 = terminal_event_digest(event)?;
        let path = self.publish(Stage::Acknowledged {
            event,
            event_sha256: event_sha256.clone(),
        })?;
        if let Some(admission) = self.admission.take() {
            admission.finish(TerminalOutcome::Acknowledged { event_sha256 })?;
        }
        Ok(path)
    }

    pub(crate) fn fail(mut self, reason: DeliveryFailure) -> Result<PathBuf> {
        let path = self.publish(Stage::Failed { reason })?;
        if let Some(admission) = self.admission.take() {
            let reason = serde_json::to_value(reason)?
                .as_str()
                .ok_or_else(|| eyre::eyre!("invalid measurement failure reason"))?
                .to_owned();
            admission.finish(TerminalOutcome::Failed { reason })?;
        }
        Ok(path)
    }

    fn publish(&self, stage: Stage<'_>) -> Result<PathBuf> {
        prepare_directory(&self.directory)?;
        let suffix = match &stage {
            Stage::Attempted => "attempt",
            Stage::Acknowledged { .. } => "ack",
            Stage::Failed { .. } => "failure",
        };
        let evidence = Evidence {
            schema_version: SCHEMA_VERSION,
            mbx_version: crate::version::VERSION,
            source_base_version: crate::version::SOURCE_BASE_VERSION,
            identity: &self.identity,
            event_id: &self.event_id,
            adapter: self.adapter,
            event_kind: self.event_kind,
            delivery: stage,
        };
        let destination = self.directory.join(format!(
            "{}.{}.measurement-{suffix}.json",
            self.identity.session_id, self.event_id
        ));
        publish_immutable(&self.directory, &destination, &evidence)?;
        Ok(destination)
    }
}

/// SHA256 over UTF-8 serde_json compact serialization of the typed event.
/// Schema field order follows MeasurementEvent; no environment or argv enters it.
pub(crate) fn terminal_event_digest(event: &MeasurementEvent) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(event)?)))
}

fn publish_immutable<T: Serialize>(directory: &Path, destination: &Path, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    let mut temporary = tempfile::Builder::new()
        .prefix(".mbx-measurement-")
        .tempfile_in(directory)?;
    temporary.write_all(&bytes)?;
    temporary.as_file_mut().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o400))?;
    }
    temporary
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    #[cfg(unix)]
    std::fs::File::open(directory)?.sync_all()?;
    Ok(())
}

fn optional_environment(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn required_environment(name: &str) -> Result<String> {
    optional_environment(name)?.ok_or_else(|| eyre::eyre!("missing measurement identity {name}"))
}

#[cfg(test)]
#[path = "measurement_reliability_tests.rs"]
mod tests;

#[cfg(all(test, unix, feature = "owned-cache-transport"))]
#[path = "measurement_reliability_admission_tests.rs"]
mod admission_tests;
