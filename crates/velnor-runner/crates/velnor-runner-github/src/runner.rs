//! Runner references returned by the Scale Set service.

use serde::Deserialize;

use crate::EncodedJit;

/// The Actions service reference to one registered runner.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunnerReference {
    /// Actions runner identifier.
    pub id: i64,
    /// Runner name.
    pub name: String,
    /// Scale set that owns the runner.
    pub runner_scale_set_id: i64,
}

/// JIT configuration and the runner reference created with it.
#[derive(Debug)]
pub struct JitResult {
    /// One-shot encoded configuration. Its debug output is redacted.
    pub encoded_jit_config: EncodedJit,
    /// Service reference to the runner created by the JIT request.
    pub runner: RunnerReference,
}
