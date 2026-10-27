// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! T62 private-home release and pressure qualification helpers.
//!
//! Included by `gc1_qualification_cases.rs` through the existing
//! `gc1_qualification` module; the `c18` through `c22` tests below exercise
//! private-home named and native commands, every test/Clippy/documentation
//! family, helper reuse with source-free authority, migration overlap, and
//! other-family pressure through real owned interfaces with local fixtures
//! only.

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_contract::{NamedCheckLane, NamedCheckLaneVariant};
use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerInvocation, CompilerLedger, DOC_PHASE_COMPILATION,
    DOC_PHASE_DOCTEST_DESCENDANT, DOC_PHASE_FINALIZATION, DOC_PHASE_PROBE, DOC_PHASES,
    OPERATION_COMPILATION, OPERATION_COMPILER_QUERY, ROLE_APPLICATION, ROLE_DEPENDENCY,
    TOOL_KIND_CLIPPY_DRIVER, TOOL_KIND_RUSTC, TOOL_KIND_RUSTDOC, TOOL_KINDS, classify_tool_kind,
    is_supported_doc_phase, is_supported_tool_kind, suggest_doc_phase,
};
use velnor_actions_mise::native_compiler_observer::{
    NativeCompletion, NativeEvent, NativeInvocation, NativeLedger, TOOL_KIND_CC, TOOL_KIND_CXX,
    is_observed_native_executable, resolve_native_tool_kind, select_native_adapter,
};
use velnor_actions_orchestrator::{
    AdmissionDecision, AdmissionRequest, BLOCKED_UNKNOWN_FOREIGN_EXPOSURE,
    BLOCKED_UNKNOWN_INVENTORY, CheckoutState, EntryKind, EntryTupleV1, FrozenInnerPayload,
    OPERATING_CEILING_BYTES, OUTER_MEMBER_NAME, OuterEnvelope, OuterEnvelopeIdentity,
    PreflightMeasurement, ProjectionEntry, RealMtime, RestoreDecision, authenticate_entry,
    build_inventory, decide, decide_admission, measure_envelope, measured_bytes, sha256_hex,
    unknown_bytes,
};

use super::cache_discovery::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
    decide_discovery, verify_restore,
};
use super::cache_intent::{
    IntentDraft, IntentObservation, IntentRecheck, derive_intent, intent_authorizes_mutation,
    recheck_intent,
};
use super::gc1_qualification_inputs::{
    LIB_ID, LINUX_TARGET, SERDE_ID, base_coverage, cold_ledger, coverage_variant, eligible_ids,
    native_cold_ledger, require_changed_coverage, require_measured_zero, require_nonzero,
    require_same_coverage, require_valid_digests, require_zero, warm_native_ledger,
};
use super::must_fail;
use crate::gc1_qualification_support::{
    FixtureCacheRef, ObservationClosure, Workload, invalid, require_complete_observation,
    require_known_case_ids, require_nonempty_workload, require_qualification_destructive,
};

#[path = "gc1_qualification_paths_a.rs"]
mod gc1_qualification_paths_a;
#[path = "gc1_qualification_paths_b.rs"]
mod gc1_qualification_paths_b;
#[path = "gc1_qualification_paths_c.rs"]
mod gc1_qualification_paths_c;
#[path = "gc1_qualification_paths_d.rs"]
mod gc1_qualification_paths_d;
#[path = "gc1_qualification_paths_shared.rs"]
mod gc1_qualification_paths_shared;
pub(crate) use gc1_qualification_paths_a::*;
pub(crate) use gc1_qualification_paths_b::*;
pub(crate) use gc1_qualification_paths_c::*;
pub(crate) use gc1_qualification_paths_d::*;
pub(crate) use gc1_qualification_paths_shared::*;
