//! Durable Scale Set creation and adoption records.

mod api;
mod storage;
#[cfg(test)]
mod tests;

use sha2::{Digest, Sha256};

use crate::config::{ConnectionId, RepositoryConnection};
use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::connection::ConnectionJournal;

/// Exact connection, repository, selector, and runner-group create request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScaleSetCreateRequest {
    connection_id: ConnectionId,
    binding_digest: String,
    repository_id: i64,
    repository: String,
    selector_name: String,
    selector_labels: Vec<String>,
    runner_group_id: i64,
}

impl ScaleSetCreateRequest {
    /// Make a request from one validated repository binding and exact group.
    pub(crate) fn new(
        binding: &RepositoryConnection,
        runner_group_id: i64,
    ) -> Result<Self, HostError> {
        let identity = binding
            .canonical_identity_bytes()
            .map_err(|_| HostError::Journal)?;
        if runner_group_id <= 0 || identity.is_empty() || identity.len() > 1_048_576 {
            return Err(HostError::Journal);
        }
        let request = Self {
            connection_id: binding.connection_id.clone(),
            binding_digest: hex_digest(&identity),
            repository_id: binding.repository_id,
            repository: binding.repository.clone(),
            selector_name: binding.scale_set.name().to_owned(),
            selector_labels: binding.scale_set.labels().to_vec(),
            runner_group_id,
        };
        request.validate()?;
        Ok(request)
    }

    /// Give the exact runner-group ID.
    #[must_use]
    pub(crate) const fn runner_group_id(&self) -> i64 {
        self.runner_group_id
    }

    /// Give the configured Scale Set name.
    #[must_use]
    pub(crate) fn selector_name(&self) -> &str {
        &self.selector_name
    }

    /// Give configured labels in registration order.
    #[must_use]
    pub(crate) fn selector_labels(&self) -> &[String] {
        &self.selector_labels
    }

    fn validate(&self) -> Result<(), HostError> {
        if self.repository_id <= 0
            || self.runner_group_id <= 0
            || self.repository.is_empty()
            || self.repository.len() > 201
            || self.selector_name.is_empty()
            || self.selector_name.len() > 64
            || self.selector_labels.len() < 2
            || self.selector_labels.len() > 64
            || self.binding_digest.len() != 64
            || !self
                .binding_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.selector_labels.first().map(String::as_str) != Some("velnor")
            || self.selector_labels.get(1).map(String::as_str) != Some(&self.selector_name)
            || self.selector_labels.iter().any(String::is_empty)
            || self
                .selector_labels
                .iter()
                .enumerate()
                .any(|(index, label)| self.selector_labels[..index].contains(label))
        {
            return Err(HostError::Journal);
        }
        Ok(())
    }
}

/// Durable Scale Set create lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScaleSetCreateState {
    /// No external create was claimed.
    Prepared,
    /// One caller claimed the create; it may be in progress.
    Requested,
    /// The create result is ambiguous and must not be repeated.
    Uncertain,
    /// The service returned the known create conflict status.
    NoEffect,
    /// A create response was verified and recorded.
    Created,
    /// An exact preexisting Scale Set was verified and recorded.
    Adopted,
}

impl ScaleSetCreateState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Requested => "requested",
            Self::Uncertain => "uncertain",
            Self::NoEffect => "no_effect",
            Self::Created => "created",
            Self::Adopted => "adopted",
        }
    }

    fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "requested" => Ok(Self::Requested),
            "uncertain" => Ok(Self::Uncertain),
            "no_effect" => Ok(Self::NoEffect),
            "created" => Ok(Self::Created),
            "adopted" => Ok(Self::Adopted),
            _ => Err(HostError::Journal),
        }
    }
}

/// Stable row identity for one connection's Scale Set lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScaleSetCreateId(i64);

/// Persisted exact remote Scale Set identity and its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScaleSetCreateReceipt {
    /// Positive service ID.
    pub(crate) id: i64,
    /// Exact configured name.
    pub(crate) name: String,
    /// Labels returned by the service.
    pub(crate) labels: Vec<String>,
    /// Whether Actions updates are disabled.
    pub(crate) disable_update: bool,
    /// Whether this host created or adopted the Scale Set.
    pub(crate) ownership: SetOwnership,
}

/// Durable prepare, replay, or final Scale Set record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScaleSetCreateRecord {
    id: ScaleSetCreateId,
    request: ScaleSetCreateRequest,
    state: ScaleSetCreateState,
    no_effect_status: Option<u16>,
    receipt: Option<ScaleSetCreateReceipt>,
}

impl ScaleSetCreateRecord {
    /// Give the durable row ID.
    #[must_use]
    pub(crate) const fn id(&self) -> ScaleSetCreateId {
        self.id
    }

    /// Give the durable lifecycle state.
    #[must_use]
    pub(crate) const fn state(&self) -> ScaleSetCreateState {
        self.state
    }

    /// Give the exact immutable request.
    #[must_use]
    pub(crate) const fn request(&self) -> &ScaleSetCreateRequest {
        &self.request
    }

    /// Give a recorded known no-effect status, if any.
    #[must_use]
    pub(crate) const fn no_effect_status(&self) -> Option<u16> {
        self.no_effect_status
    }

    /// Give the recorded remote identity, if created or adopted.
    #[must_use]
    pub(crate) const fn receipt(&self) -> Option<&ScaleSetCreateReceipt> {
        self.receipt.as_ref()
    }
}

/// One-shot in-memory authority to finish the claimed create call.
#[derive(Debug)]
pub(crate) struct ScaleSetCreateClaim {
    journal: Journal,
    id: ScaleSetCreateId,
    request: ScaleSetCreateRequest,
}

impl ScaleSetCreateClaim {
    fn new(scope: &ConnectionJournal, record: &ScaleSetCreateRecord) -> Self {
        Self {
            journal: scope.journal_ref().clone(),
            id: record.id,
            request: record.request.clone(),
        }
    }
}

/// A create response that passed exact request checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreatedScaleSetResponse(VerifiedScaleSet);

impl CreatedScaleSetResponse {
    /// Record a create response only when it matches the exact request.
    pub(crate) fn verified(
        request: &ScaleSetCreateRequest,
        id: i64,
        name: &str,
        labels: &[String],
        disable_update: bool,
    ) -> Result<Self, HostError> {
        Ok(Self(VerifiedScaleSet::new(
            request,
            id,
            name,
            labels,
            disable_update,
        )?))
    }
}

/// A read-only GET observation of an existing exact Scale Set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExistingScaleSetObservation(VerifiedScaleSet);

impl ExistingScaleSetObservation {
    /// Record only an exact existing-set observation.
    pub(crate) fn verified(
        request: &ScaleSetCreateRequest,
        id: i64,
        name: &str,
        labels: &[String],
        disable_update: bool,
    ) -> Result<Self, HostError> {
        Ok(Self(VerifiedScaleSet::new(
            request,
            id,
            name,
            labels,
            disable_update,
        )?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifiedScaleSet {
    request: ScaleSetCreateRequest,
    id: i64,
    name: String,
    labels: Vec<String>,
    disable_update: bool,
}

impl VerifiedScaleSet {
    fn new(
        request: &ScaleSetCreateRequest,
        id: i64,
        name: &str,
        labels: &[String],
        disable_update: bool,
    ) -> Result<Self, HostError> {
        request.validate()?;
        let mut actual = labels.to_vec();
        let mut expected = request.selector_labels.clone();
        actual.sort_unstable();
        expected.sort_unstable();
        if id <= 0
            || name != request.selector_name
            || !disable_update
            || actual != expected
            || actual.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            request: request.clone(),
            id,
            name: name.to_owned(),
            labels: labels.to_vec(),
            disable_update,
        })
    }

    fn receipt(&self, ownership: SetOwnership) -> ScaleSetCreateReceipt {
        ScaleSetCreateReceipt {
            id: self.id,
            name: self.name.clone(),
            labels: self.labels.clone(),
            disable_update: self.disable_update,
            ownership,
        }
    }
}

/// Typed proof that the create endpoint returned its known conflict status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScaleSetCreateNoEffect(u16);

impl ScaleSetCreateNoEffect {
    /// Accept the pinned create-conflict status only.
    pub(crate) fn from_http_status(status: u16) -> Result<Self, HostError> {
        if status == 409 {
            Ok(Self(status))
        } else {
            Err(HostError::Identity)
        }
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    output
}
