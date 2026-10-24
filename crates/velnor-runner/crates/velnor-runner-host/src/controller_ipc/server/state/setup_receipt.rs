//! Keep the latest verified setup facts under the observation lifecycle lock.

use std::collections::BTreeMap;
use std::io::{self, Write};

use crate::config::{HostConfig, RepositoryConnection};
use crate::error::HostError;
use crate::ipc::MAX_FRAME;
use crate::launch::steady::{PollObservation, PollStatistics};
use crate::scale_set::EnsuredSet;
use serde::Serialize;
use velnor_runner_github::RunnerGroup;

use super::super::super::wire::{
    ConnectionCensusReply, ConnectionSetupSnapshot, Response, RunnerGroupSnapshot,
    SelectedSetSnapshot, SessionObservationReply, SessionObservationStatus, SetupReceiptReply,
    SetupReceiptStatus,
};
use super::session_observation::SessionObservationCoordinator;
use super::{ControlState, binding_sha256, session_observation};

#[derive(Debug, Clone)]
pub(crate) struct SetupReceiptInput {
    pub(crate) binding: RepositoryConnection,
    pub(crate) set: EnsuredSet,
    pub(crate) groups: Vec<RunnerGroup>,
    pub(crate) captured_at_unix_ms: u64,
    pub(crate) session_intent_id: i64,
    pub(crate) session_id: String,
    pub(crate) engine_id: String,
    pub(crate) docker_root_digest: String,
}

#[derive(Debug, Clone)]
pub(super) struct SetupReceiptRecord {
    pub(super) binding_sha256: String,
    pub(super) config_generation: u64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) scale_set_id: i64,
    pub(super) session_intent_id: i64,
    pub(super) session_id: String,
    pub(super) snapshot: ConnectionSetupSnapshot,
}

impl SetupReceiptRecord {
    pub(super) fn create(
        input: SetupReceiptInput,
        config_generation: u64,
        revision: u64,
    ) -> Result<Self, HostError> {
        validate_input(&input)?;
        let SetupReceiptInput {
            binding,
            set,
            groups,
            captured_at_unix_ms,
            session_intent_id,
            session_id,
            engine_id,
            docker_root_digest,
        } = input;
        let binding_sha256 = binding_sha256(&binding)?;
        let scale_set_id = set.id;
        let snapshot = ConnectionSetupSnapshot {
            revision,
            captured_at_unix_ms,
            runner_groups: groups
                .into_iter()
                .map(|group| RunnerGroupSnapshot {
                    id: group.id,
                    name: group.name,
                    is_default: group.is_default,
                })
                .collect(),
            selected_set: SelectedSetSnapshot {
                id: set.id,
                name: set.name,
                runner_group_id: set.runner_group_id,
                disable_update: set.disable_update,
                labels: set.labels,
            },
        };
        ensure_snapshot_fits(&snapshot)?;
        Ok(Self {
            binding_sha256,
            config_generation,
            engine_id,
            docker_root_digest,
            scale_set_id,
            session_intent_id,
            session_id,
            snapshot,
        })
    }

    pub(super) fn matches(
        &self,
        binding_sha256: &str,
        config_generation: u64,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> bool {
        self.binding_sha256 == binding_sha256
            && self.config_generation == config_generation
            && self.engine_id == engine_id
            && self.docker_root_digest == docker_root_digest
    }
}

fn validate_input(input: &SetupReceiptInput) -> Result<(), HostError> {
    if input.set.id <= 0
        || input.set.name != input.binding.scale_set.name()
        || input.set.labels.as_slice() != input.binding.scale_set.labels()
        || input.set.runner_group_id <= 0
        || input.session_intent_id <= 0
        || input.session_id.trim().is_empty()
        || input.captured_at_unix_ms == 0
        || !session_observation::valid_engine(&input.engine_id)
        || !session_observation::valid_digest(&input.docker_root_digest)
    {
        return Err(HostError::Identity);
    }
    let mut defaults = input.groups.iter().filter(|group| group.is_default);
    let Some(default) = defaults.next() else {
        return Err(HostError::Identity);
    };
    if default.id != input.set.runner_group_id || defaults.next().is_some() {
        return Err(HostError::Identity);
    }
    if input
        .groups
        .iter()
        .any(|group| group.id <= 0 || group.name.is_empty())
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn ensure_snapshot_fits(snapshot: &ConnectionSetupSnapshot) -> Result<(), HostError> {
    ensure_json_fits(snapshot)?;
    ensure_json_fits(&setup_response(snapshot.clone()))?;
    ensure_json_fits(&census_response(snapshot.clone()))
}

fn setup_response(snapshot: ConnectionSetupSnapshot) -> Response {
    Response {
        value: None,
        doctor_probe: None,
        readiness: None,
        session_observation: None,
        setup_receipt: Some(SetupReceiptReply {
            status: SetupReceiptStatus::Ready,
            setup: Some(snapshot),
        }),
        connection_census: None,
    }
}

fn census_response(snapshot: ConnectionSetupSnapshot) -> Response {
    Response {
        value: None,
        doctor_probe: None,
        readiness: None,
        session_observation: None,
        setup_receipt: None,
        connection_census: Some(ConnectionCensusReply {
            setup_status: SetupReceiptStatus::Ready,
            setup: Some(snapshot),
            observation: SessionObservationReply {
                status: SessionObservationStatus::Complete,
                poll: Some(PollObservation::Complete {
                    available_events: u32::MAX,
                    statistics: Some(PollStatistics {
                        available_jobs: i64::MIN,
                        acquired_jobs: i64::MIN,
                        assigned_jobs: i64::MIN,
                        running_jobs: i64::MIN,
                        registered_runners: i64::MIN,
                        busy_runners: i64::MIN,
                        idle_runners: i64::MIN,
                    }),
                }),
                observed_at_unix_ms: Some(u64::MAX),
            },
        }),
    }
}

fn ensure_json_fits<T: Serialize>(value: &T) -> Result<(), HostError> {
    let mut counter = FrameSizeCounter::default();
    serde_json::to_writer(&mut counter, value).map_err(|_| HostError::Frame)
}

#[derive(Default)]
struct FrameSizeCounter {
    bytes: usize,
}

impl Write for FrameSizeCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("response exceeds frame size"))?;
        if next > MAX_FRAME {
            return Err(io::Error::other("response exceeds frame size"));
        }
        self.bytes = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ControlState {
    pub(crate) fn replace_config_with_setup(
        &self,
        config: &HostConfig,
        inputs: Vec<SetupReceiptInput>,
    ) -> Result<(), HostError> {
        let mut observation = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        observation.replace_config(config.connections())?;
        *self.config.write().map_err(|_| HostError::Journal)? = Some(config.clone());
        *self.bindings.write().map_err(|_| HostError::Journal)? = config.connections().to_vec();
        self.session_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .clear();
        self.pending_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .clear();
        self.clear_readiness()?;
        self.clear_readiness_sample()?;
        Self::replace_setup_receipts_locked(&mut observation, inputs, config.connections())
    }

    pub(crate) fn replace_setup_receipts(
        &self,
        inputs: Vec<SetupReceiptInput>,
    ) -> Result<(), HostError> {
        let mut observation = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        let bindings = self.bindings.read().map_err(|_| HostError::Journal)?;
        self.session_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .clear();
        self.pending_polls
            .write()
            .map_err(|_| HostError::Journal)?
            .clear();
        self.clear_readiness()?;
        self.clear_readiness_sample()?;
        Self::replace_setup_receipts_locked(&mut observation, inputs, &bindings)
    }

    pub(in crate::controller_ipc::server) fn setup_receipt_status(
        &self,
        connection_id: &str,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Result<SetupReceiptReply, HostError> {
        if !session_observation::valid_engine(engine_id)
            || !session_observation::valid_digest(docker_root_digest)
        {
            return Err(HostError::Frame);
        }
        let observation = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        if !self.admission_ready() {
            return Ok(setup_receipt_reply(SetupReceiptStatus::Stale, None));
        }
        let Some(binding) = self.current_binding(connection_id)? else {
            return Ok(setup_receipt_reply(SetupReceiptStatus::Stale, None));
        };
        let digest = binding_sha256(&binding)?;
        let Some(receipt) = observation.setup_receipts.get(connection_id) else {
            return Ok(setup_receipt_reply(SetupReceiptStatus::Pending, None));
        };
        if !receipt.matches(
            &digest,
            observation.config_generation,
            engine_id,
            docker_root_digest,
        ) {
            return Ok(setup_receipt_reply(SetupReceiptStatus::Stale, None));
        }
        Ok(setup_receipt_reply(
            SetupReceiptStatus::Ready,
            Some(receipt.snapshot.clone()),
        ))
    }

    fn replace_setup_receipts_locked(
        observation: &mut SessionObservationCoordinator,
        inputs: Vec<SetupReceiptInput>,
        bindings: &[RepositoryConnection],
    ) -> Result<(), HostError> {
        let revision = observation
            .setup_revision
            .checked_add(1)
            .ok_or(HostError::Journal)?;
        let mut receipts = BTreeMap::new();
        for input in inputs {
            if !bindings.iter().any(|binding| binding == &input.binding) {
                return Err(HostError::Config);
            }
            let connection_id = input.binding.connection_id.as_str().to_owned();
            let receipt =
                SetupReceiptRecord::create(input, observation.config_generation, revision)?;
            if receipts.insert(connection_id, receipt).is_some() {
                return Err(HostError::Config);
            }
        }
        observation.setup_revision = revision;
        observation.setup_receipts = receipts;
        observation.invalidate_all();
        Ok(())
    }
}

fn setup_receipt_reply(
    status: SetupReceiptStatus,
    setup: Option<ConnectionSetupSnapshot>,
) -> SetupReceiptReply {
    SetupReceiptReply { status, setup }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_limit_uses_actual_setup_and_census_json_envelopes() {
        let accepted = snapshot("g".repeat(180_000));
        assert!(ensure_snapshot_fits(&accepted).is_ok());

        let rejected = snapshot("g".repeat(MAX_FRAME));
        assert!(matches!(
            ensure_snapshot_fits(&rejected),
            Err(HostError::Frame)
        ));
    }

    fn snapshot(group_name: String) -> ConnectionSetupSnapshot {
        ConnectionSetupSnapshot {
            revision: 1,
            captured_at_unix_ms: 1_700_000_000_000,
            runner_groups: vec![RunnerGroupSnapshot {
                id: 7,
                name: group_name,
                is_default: true,
            }],
            selected_set: SelectedSetSnapshot {
                id: 17,
                name: "verified-set".to_owned(),
                runner_group_id: 7,
                disable_update: true,
                labels: Vec::new(),
            },
        }
    }
}
