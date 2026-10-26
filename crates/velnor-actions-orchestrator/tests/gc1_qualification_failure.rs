// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! T61 publication and recovery qualification helpers.
//!
//! Included by `gc1_qualification_cases.rs` through the existing
//! `gc1_qualification` module; the `c13` through `c17` tests below exercise
//! skipped and failed producers, denied saves, damaged caches, budget
//! admission, and coverage growth through real owned interfaces with local
//! fixtures only.

use std::error::Error;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::{
    JobBindingV1, MeasuredU64, ObservationGapCode, ObservationGapV1,
};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerInvocation, CompilerLedger, OPERATION_COMPILATION,
    ROLE_APPLICATION, ROLE_DEPENDENCY, TOOL_KIND_RUSTC,
};
use velnor_actions_orchestrator::cache_publication::{
    PUBLICATION_SCHEMA, PublicationOutcome, PublicationV1, ReadbackOutcome, SaveHalt, SaveOutcome,
    SaveRequest, classify_publication, decide_save,
};
use velnor_actions_orchestrator::cache_recovery::{
    ObservedEntry, ReadbackInputs, ReadbackVerdict, decide_readback, require_empty_restore_target,
    retain_usable_families, sha256_hex,
};
use velnor_actions_orchestrator::{
    AdmissionDecision, AdmissionRequest, BLOCKED_GENERATION_SLOTS_EXHAUSTED,
    BLOCKED_INSUFFICIENT_HEADROOM, BLOCKED_INSUFFICIENT_LOCAL_DISK,
    BLOCKED_UNKNOWN_FOREIGN_EXPOSURE, BLOCKED_UNKNOWN_INVENTORY, BLOCKED_UNKNOWN_LOCAL_DISK,
    BLOCKED_UNKNOWN_PENDING_WRITES, BLOCKED_UNKNOWN_RUNTIME_ENVELOPE, EntryTupleV1,
    HARD_CEILING_BYTES, MAX_GENERATION_SLOTS, OPERATING_CEILING_BYTES, OUTER_MEMBER_NAME,
    OuterEnvelope, OuterEnvelopeIdentity, PreflightMeasurement, UNKNOWN_HOSTED_PACKAGING,
    build_inventory, decide_admission, inventory_digest, measure_envelope, measured_bytes,
    unknown_bytes, unknown_envelope,
};

use super::cache_coverage_replay::{
    FrozenArchive, FrozenProofV1, MbxInitialState, PairedInputBinding, QualificationOutcome,
    freeze_archive, verify_replay_frozen,
};
use super::cache_discovery::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
    decide_discovery, select_exact, select_predecessor, verify_restore,
};
use super::cache_intent::{
    IntentDraft, IntentObservation, IntentRecheck, derive_intent, intent_authorizes_mutation,
    intent_canonical_bytes, intent_digest, parse_intent_bytes, recheck_intent,
};
use super::compiler_observation::{
    ChildOutcome, ObservationReport, ObservationSession, new_session_id,
};
use super::gc1_qualification_inputs::{
    APP_ID, LIB_ID, LINUX_TARGET, base_coverage, changed_closure_ledger, cold_ledger,
    coverage_variant, eligible_ids, require_changed_coverage, require_measured_zero,
    require_nonzero, require_same_coverage, require_valid_digests, require_zero,
    warm_native_ledger,
};
use super::must_fail;
use crate::gc1_qualification_support::{
    ObservationClosure, Workload, invalid, require_complete_observation, require_known_case_ids,
    require_nonempty_workload, require_qualification_destructive,
};
pub(crate) fn check_failure_harness(case_id: &str, detail: &str) -> Result<(), Box<dyn Error>> {
    require_known_case_ids(&[case_id.to_owned()])?;
    require_nonempty_workload(&Workload {
        recipe_ids: vec!["check".to_owned(), "clippy".to_owned(), "test".to_owned()],
        eligible_dependency_ids: eligible_ids(),
    })?;
    require_complete_observation(&ObservationClosure {
        complete: true,
        detail: detail.to_owned(),
    })?;
    Ok(())
}

#[path = "gc1_qualification_failure_a.rs"]
mod gc1_qualification_failure_a;
#[path = "gc1_qualification_failure_b.rs"]
mod gc1_qualification_failure_b;
#[path = "gc1_qualification_failure_c.rs"]
mod gc1_qualification_failure_c;
#[path = "gc1_qualification_failure_d.rs"]
mod gc1_qualification_failure_d;
#[path = "gc1_qualification_failure_e.rs"]
mod gc1_qualification_failure_e;
#[path = "gc1_qualification_failure_f.rs"]
mod gc1_qualification_failure_f;
#[path = "gc1_qualification_failure_g.rs"]
mod gc1_qualification_failure_g;
#[path = "gc1_qualification_failure_shared.rs"]
mod gc1_qualification_failure_shared;
pub(crate) use gc1_qualification_failure_a::*;
pub(crate) use gc1_qualification_failure_b::*;
pub(crate) use gc1_qualification_failure_c::*;
pub(crate) use gc1_qualification_failure_d::*;
pub(crate) use gc1_qualification_failure_e::*;
pub(crate) use gc1_qualification_failure_f::*;
pub(crate) use gc1_qualification_failure_g::*;
pub(crate) use gc1_qualification_failure_shared::*;
