//! Runner references returned by the pinned Scale Set service.

use serde::Deserialize;

/// One runner reference from the distributed-task API.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunnerReference {
    /// Actions runner identifier.
    pub id: i64,
    /// Exact runner name.
    pub name: String,
    /// Owning runner scale set identifier.
    pub runner_scale_set_id: i64,
}
