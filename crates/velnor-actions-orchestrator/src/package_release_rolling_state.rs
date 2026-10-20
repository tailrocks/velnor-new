//! Canonical durable journal envelope for the one mutable preview release.

use crate::{OrchestratorError, internal::internal};
use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    canonical_json_bytes,
    publication_retry::rolling::{RollingBootstrapSnapshot, RollingJournal},
};

const PREFIX: &str = "velnor-rolling-preview-v1\n";
const MAX_BODY: usize = 110 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    pub(super) schema: String,
    pub(super) repository: String,
    pub(super) repository_id: u64,
    pub(super) release_id: u64,
    pub(super) tag: String,
    pub(super) journal: RollingJournal,
    pub(super) predecessor: Option<Predecessor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "evidence",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub(super) enum Predecessor {
    Bootstrap(RollingBootstrapSnapshot),
    Native(Box<Envelope>),
}

impl Envelope {
    pub(super) fn body(&self) -> Result<String, OrchestratorError> {
        let bytes = canonical_json_bytes(self).map_err(|_| internal("rolling_journal_encoding"))?;
        let json = std::str::from_utf8(&bytes).map_err(|_| internal("rolling_journal_utf8"))?;
        let body = format!("{PREFIX}{json}");
        if body.len() > MAX_BODY {
            return Err(internal("rolling_journal_size"));
        }
        Ok(body)
    }

    pub(super) fn parse(body: Option<&str>) -> Result<Option<Self>, OrchestratorError> {
        let Some(body) = body else { return Ok(None) };
        let Some(json) = body.strip_prefix(PREFIX) else {
            if body.contains("velnor-rolling-preview") {
                return Err(internal("rolling_journal_prefix"));
            }
            return Ok(None);
        };
        if body.len() > MAX_BODY {
            return Err(internal("rolling_journal_size"));
        }
        let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(
            json.as_bytes(),
            MAX_BODY,
        )
        .map_err(|_| internal("rolling_journal_json"))?;
        let envelope: Self =
            serde_json::from_value(value).map_err(|_| internal("rolling_journal_schema"))?;
        if envelope.body()? != body {
            return Err(internal("rolling_journal_noncanonical"));
        }
        Ok(Some(envelope))
    }
}
