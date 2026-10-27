// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::workflow::cache_context::{
    MeasuredU64, ObservationGapCode, ObservationGapV1,
};

use super::validation::{
    ObservationError, check_binding, check_context, check_detail, check_session_id, measured,
    reject,
};

pub(crate) const OBSERVATION_SCHEMA: u32 = 1;

static NEXT_SESSION: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildOutcome {
    Success,
    Failure,
}

#[must_use]
pub(crate) fn new_session_id() -> String {
    format!(
        "check-obs-{}-{}",
        std::process::id(),
        NEXT_SESSION.fetch_add(1, Ordering::Relaxed)
    )
}

pub(super) fn epoch_ms_now() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservationSession {
    pub(crate) recipe_id: String,
    pub(crate) check_id: String,
    pub(crate) session_id: String,
    pub(crate) context_digest: String,
    pub(crate) started_epoch_ms: u64,
    pub(crate) compiler: Option<CompilerEvidence>,
    pub(crate) native: Option<NativeEvidence>,
    pub(crate) origin: Option<OriginEvidence>,
    pub(crate) cache: Option<CacheEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompilerEvidence {
    pub(crate) session_id: String,
    pub(crate) dependency_compilations: MeasuredU64,
    pub(crate) application_compilations: MeasuredU64,
    pub(crate) compiler_queries: MeasuredU64,
    pub(crate) failed_calls: MeasuredU64,
    pub(crate) interrupted_calls: MeasuredU64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeEvidence {
    pub(crate) session_id: String,
    pub(crate) native_compilations: MeasuredU64,
    pub(crate) native_queries: MeasuredU64,
    pub(crate) failed_calls: MeasuredU64,
    pub(crate) interrupted_calls: MeasuredU64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginEvidence {
    pub(crate) session_id: String,
    pub(crate) origin_acquisitions: MeasuredU64,
    pub(crate) origin_bytes: MeasuredU64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CacheEvidence {
    pub(crate) session_id: String,
    pub(crate) cache_bytes: MeasuredU64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservationReport {
    pub(crate) schema: u32,
    pub(crate) recipe_id: String,
    pub(crate) check_id: String,
    pub(crate) session_id: String,
    pub(crate) execution_context_digest: String,
    pub(crate) child_failed: bool,
    pub(crate) started_epoch_ms: u64,
    pub(crate) finished_epoch_ms: u64,
    pub(crate) observation_duration_ms: u64,
    pub(crate) dependency_compilations: MeasuredU64,
    pub(crate) application_compilations: MeasuredU64,
    pub(crate) compiler_queries: MeasuredU64,
    pub(crate) compiler_failed_calls: MeasuredU64,
    pub(crate) compiler_interrupted_calls: MeasuredU64,
    pub(crate) native_compilations: MeasuredU64,
    pub(crate) native_queries: MeasuredU64,
    pub(crate) native_failed_calls: MeasuredU64,
    pub(crate) native_interrupted_calls: MeasuredU64,
    pub(crate) origin_acquisitions: MeasuredU64,
    pub(crate) origin_bytes: MeasuredU64,
    pub(crate) cache_bytes: MeasuredU64,
    pub(crate) gaps: Vec<ObservationGapV1>,
}

impl ObservationSession {
    pub(crate) fn begin(
        recipe_id: &str,
        check_id: &str,
        session_id: &str,
        context_digest: &str,
    ) -> Result<Self, ObservationError> {
        check_binding("recipe_id", recipe_id)?;
        check_binding("check_id", check_id)?;
        check_session_id(session_id)?;
        check_context(context_digest)?;
        Ok(Self {
            recipe_id: recipe_id.to_owned(),
            check_id: check_id.to_owned(),
            session_id: session_id.to_owned(),
            context_digest: context_digest.to_owned(),
            started_epoch_ms: epoch_ms_now(),
            compiler: None,
            native: None,
            origin: None,
            cache: None,
        })
    }

    fn gap(&self, code: ObservationGapCode, detail: &str) -> ObservationGapV1 {
        ObservationGapV1 {
            code,
            detail: detail.to_owned(),
            affected_recipe_ids: vec![self.recipe_id.clone()],
            evidence_ref: self.session_id.clone(),
        }
    }

    pub(crate) fn compiler_evidence(
        &self,
        dependency: Option<u64>,
        application: Option<u64>,
        queries: Option<u64>,
        failed: Option<u64>,
        interrupted: Option<u64>,
        unknown_detail: &str,
    ) -> Result<CompilerEvidence, ObservationError> {
        check_detail(unknown_detail)?;
        match (dependency, application, queries, failed, interrupted) {
            (
                Some(dependency),
                Some(application),
                Some(queries),
                Some(failed),
                Some(interrupted),
            ) => Ok(CompilerEvidence {
                session_id: self.session_id.clone(),
                dependency_compilations: measured(dependency),
                application_compilations: measured(application),
                compiler_queries: measured(queries),
                failed_calls: measured(failed),
                interrupted_calls: measured(interrupted),
            }),
            (None, None, None, None, None) => {
                let gap = self.gap(ObservationGapCode::IncompleteAttribution, unknown_detail);
                let unknown = || MeasuredU64 {
                    value: None,
                    unknown_reason: Some(gap.clone()),
                };
                Ok(CompilerEvidence {
                    session_id: self.session_id.clone(),
                    dependency_compilations: unknown(),
                    application_compilations: unknown(),
                    compiler_queries: unknown(),
                    failed_calls: unknown(),
                    interrupted_calls: unknown(),
                })
            }
            _ => Err(reject("compiler_evidence", "partial_dimension")),
        }
    }

    pub(crate) fn native_evidence(
        &self,
        compilations: Option<u64>,
        queries: Option<u64>,
        failed: Option<u64>,
        interrupted: Option<u64>,
        unknown_detail: &str,
    ) -> Result<NativeEvidence, ObservationError> {
        check_detail(unknown_detail)?;
        match (compilations, queries, failed, interrupted) {
            (Some(compilations), Some(queries), Some(failed), Some(interrupted)) => {
                Ok(NativeEvidence {
                    session_id: self.session_id.clone(),
                    native_compilations: measured(compilations),
                    native_queries: measured(queries),
                    failed_calls: measured(failed),
                    interrupted_calls: measured(interrupted),
                })
            }
            (None, None, None, None) => {
                let gap = self.gap(ObservationGapCode::UnknownNativeCoverage, unknown_detail);
                let unknown = || MeasuredU64 {
                    value: None,
                    unknown_reason: Some(gap.clone()),
                };
                Ok(NativeEvidence {
                    session_id: self.session_id.clone(),
                    native_compilations: unknown(),
                    native_queries: unknown(),
                    failed_calls: unknown(),
                    interrupted_calls: unknown(),
                })
            }
            _ => Err(reject("native_evidence", "partial_dimension")),
        }
    }

    pub(crate) fn origin_evidence(
        &self,
        acquisitions: Option<u64>,
        bytes: Option<u64>,
        unknown_detail: &str,
    ) -> Result<OriginEvidence, ObservationError> {
        check_detail(unknown_detail)?;
        match (acquisitions, bytes) {
            (Some(acquisitions), Some(bytes)) => Ok(OriginEvidence {
                session_id: self.session_id.clone(),
                origin_acquisitions: measured(acquisitions),
                origin_bytes: measured(bytes),
            }),
            (None, None) => {
                let gap = self.gap(ObservationGapCode::UnobservedNetworkAccess, unknown_detail);
                let unknown = || MeasuredU64 {
                    value: None,
                    unknown_reason: Some(gap.clone()),
                };
                Ok(OriginEvidence {
                    session_id: self.session_id.clone(),
                    origin_acquisitions: unknown(),
                    origin_bytes: unknown(),
                })
            }
            _ => Err(reject("origin_evidence", "partial_dimension")),
        }
    }

    pub(crate) fn cache_evidence(
        &self,
        bytes: Option<u64>,
        unknown_detail: &str,
    ) -> Result<CacheEvidence, ObservationError> {
        check_detail(unknown_detail)?;
        if let Some(bytes) = bytes {
            Ok(CacheEvidence {
                session_id: self.session_id.clone(),
                cache_bytes: measured(bytes),
            })
        } else {
            let gap = self.gap(ObservationGapCode::IncompleteAttribution, unknown_detail);
            Ok(CacheEvidence {
                session_id: self.session_id.clone(),
                cache_bytes: MeasuredU64 {
                    value: None,
                    unknown_reason: Some(gap),
                },
            })
        }
    }

    pub(crate) fn provide_compiler(
        &mut self,
        evidence: CompilerEvidence,
    ) -> Result<(), ObservationError> {
        if evidence.session_id != self.session_id {
            return Err(reject("compiler_evidence", "foreign_session"));
        }
        if self.compiler.is_some() {
            return Err(reject("compiler_evidence", "duplicate_dimension"));
        }
        self.compiler = Some(evidence);
        Ok(())
    }

    pub(crate) fn provide_native(
        &mut self,
        evidence: NativeEvidence,
    ) -> Result<(), ObservationError> {
        if evidence.session_id != self.session_id {
            return Err(reject("native_evidence", "foreign_session"));
        }
        if self.native.is_some() {
            return Err(reject("native_evidence", "duplicate_dimension"));
        }
        self.native = Some(evidence);
        Ok(())
    }

    pub(crate) fn provide_origin(
        &mut self,
        evidence: OriginEvidence,
    ) -> Result<(), ObservationError> {
        if evidence.session_id != self.session_id {
            return Err(reject("origin_evidence", "foreign_session"));
        }
        if self.origin.is_some() {
            return Err(reject("origin_evidence", "duplicate_dimension"));
        }
        self.origin = Some(evidence);
        Ok(())
    }

    pub(crate) fn provide_cache(
        &mut self,
        evidence: CacheEvidence,
    ) -> Result<(), ObservationError> {
        if evidence.session_id != self.session_id {
            return Err(reject("cache_evidence", "foreign_session"));
        }
        if self.cache.is_some() {
            return Err(reject("cache_evidence", "duplicate_dimension"));
        }
        self.cache = Some(evidence);
        Ok(())
    }
}
