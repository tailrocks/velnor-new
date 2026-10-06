//! Canonical bounded predecessor-admission wire envelope.

use serde::Serialize;
use velnor_actions_contract::QualificationSourceDelta;

use super::api::FetchedNode;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionDocument {
    pub(super) predecessor: FetchedNode,
    pub(super) source_delta: Option<QualificationSourceDelta>,
}
