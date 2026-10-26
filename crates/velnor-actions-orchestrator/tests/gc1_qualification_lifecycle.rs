// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! T60 event and cancellation qualification helpers.
//!
//! Included by `gc1_qualification_cases.rs` through the existing
//! `gc1_qualification` module; the `c07` through `c12` tests below exercise
//! event trust, competing producers, cancellation phases, and mixed-attempt
//! reruns through real owned interfaces with local fixtures only.

use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::canonical::{Digest, digest_b3, digest_b3_typed};
use velnor_actions_contract::workflow::cache_context::{
    JobBindingV1, MeasuredU64, ObservationGapCode, ObservationGapV1,
};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_contract::workflow::ir::{
    CacheMode, Concurrency, WRITER_QUEUE_CANCEL_IN_PROGRESS,
};
use velnor_actions_contract::workflow::plan::WorkflowEvent;
use velnor_actions_contract::workflow::trust::{Trust, trust_for_event};
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

use super::cache_coverage_replay::{
    FrozenArchive, FrozenProofV1, MbxInitialState, PairedInputBinding, QualificationOutcome,
    freeze_archive, verify_replay_frozen,
};
use super::cache_discovery::{
    DiscoveryDecision, DiscoveryRequest, EntryProvenance, RestoreObservation, SelectableEntry,
    ShadowState, classify_shadow, decide_discovery, primary_prefix, select_exact,
    select_predecessor, verify_restore,
};
use super::compiler_observation::{ChildOutcome, ObservationSession, new_session_id};
use super::gc1_qualification_inputs::{
    LINUX_TARGET, cold_ledger, coverage_variant, eligible_ids, require_changed_coverage,
    require_same_coverage,
};
use super::must_fail;
use crate::gc1_qualification_support::{
    ObservationClosure, Workload, invalid, require_complete_observation, require_known_case_ids,
    require_nonempty_workload,
};

#[path = "gc1_qualification_lifecycle_a.rs"]
mod gc1_qualification_lifecycle_a;
#[path = "gc1_qualification_lifecycle_b.rs"]
mod gc1_qualification_lifecycle_b;
#[path = "gc1_qualification_lifecycle_c.rs"]
mod gc1_qualification_lifecycle_c;
#[path = "gc1_qualification_lifecycle_d.rs"]
mod gc1_qualification_lifecycle_d;
#[path = "gc1_qualification_lifecycle_shared.rs"]
mod gc1_qualification_lifecycle_shared;
pub(crate) use gc1_qualification_lifecycle_a::*;
pub(crate) use gc1_qualification_lifecycle_b::*;
pub(crate) use gc1_qualification_lifecycle_c::*;
pub(crate) use gc1_qualification_lifecycle_d::*;
pub(crate) use gc1_qualification_lifecycle_shared::*;
