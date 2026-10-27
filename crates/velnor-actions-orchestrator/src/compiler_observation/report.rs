// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::session::{
    ChildOutcome, OBSERVATION_SCHEMA, ObservationReport, ObservationSession, epoch_ms_now,
};
use super::validation::{ObservationError, check_binding, check_context, check_session_id, reject};

impl ObservationSession {
    pub(crate) fn finish(
        self,
        outcome: ChildOutcome,
    ) -> Result<ObservationReport, ObservationError> {
        let compiler = self
            .compiler
            .ok_or_else(|| reject("observation", "missing_compiler_evidence"))?;
        let native = self
            .native
            .ok_or_else(|| reject("observation", "missing_native_evidence"))?;
        let origin = self
            .origin
            .ok_or_else(|| reject("observation", "missing_origin_evidence"))?;
        let cache = self
            .cache
            .ok_or_else(|| reject("observation", "missing_cache_evidence"))?;
        let mut gaps = Vec::new();
        for measured in [
            &compiler.dependency_compilations,
            &compiler.application_compilations,
            &compiler.compiler_queries,
            &compiler.failed_calls,
            &compiler.interrupted_calls,
            &native.native_compilations,
            &native.native_queries,
            &native.failed_calls,
            &native.interrupted_calls,
            &origin.origin_acquisitions,
            &origin.origin_bytes,
            &cache.cache_bytes,
        ] {
            if let Some(reason) = &measured.unknown_reason
                && !gaps.contains(reason)
            {
                gaps.push(reason.clone());
            }
        }
        let finished_epoch_ms = epoch_ms_now();
        let report = ObservationReport {
            schema: OBSERVATION_SCHEMA,
            recipe_id: self.recipe_id,
            check_id: self.check_id,
            session_id: self.session_id,
            execution_context_digest: self.context_digest,
            child_failed: outcome == ChildOutcome::Failure,
            started_epoch_ms: self.started_epoch_ms,
            finished_epoch_ms,
            observation_duration_ms: finished_epoch_ms.saturating_sub(self.started_epoch_ms),
            dependency_compilations: compiler.dependency_compilations,
            application_compilations: compiler.application_compilations,
            compiler_queries: compiler.compiler_queries,
            compiler_failed_calls: compiler.failed_calls,
            compiler_interrupted_calls: compiler.interrupted_calls,
            native_compilations: native.native_compilations,
            native_queries: native.native_queries,
            native_failed_calls: native.failed_calls,
            native_interrupted_calls: native.interrupted_calls,
            origin_acquisitions: origin.origin_acquisitions,
            origin_bytes: origin.origin_bytes,
            cache_bytes: cache.cache_bytes,
            gaps,
        };
        report.validate()?;
        Ok(report)
    }
}

impl ObservationReport {
    pub(crate) fn validate(&self) -> Result<(), ObservationError> {
        if self.schema != OBSERVATION_SCHEMA {
            return Err(reject("schema", "unsupported"));
        }
        check_binding("recipe_id", &self.recipe_id)?;
        check_binding("check_id", &self.check_id)?;
        check_session_id(&self.session_id)?;
        check_context(&self.execution_context_digest)?;
        for measured in [
            &self.dependency_compilations,
            &self.application_compilations,
            &self.compiler_queries,
            &self.compiler_failed_calls,
            &self.compiler_interrupted_calls,
            &self.native_compilations,
            &self.native_queries,
            &self.native_failed_calls,
            &self.native_interrupted_calls,
            &self.origin_acquisitions,
            &self.origin_bytes,
            &self.cache_bytes,
        ] {
            measured
                .validate()
                .map_err(|error| reject("summary", &error.to_string()))?;
        }
        for gap in &self.gaps {
            gap.validate()
                .map_err(|error| reject("gaps", &error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn to_canonical_json(&self) -> Result<Vec<u8>, ObservationError> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|error| reject("report", &error.to_string()))
    }
}
