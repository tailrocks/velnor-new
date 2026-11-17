// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) mod adapter;
mod cleanup;
pub(crate) mod containers;
mod delivery;
mod pre_jit_recovery;
mod publication;
mod storage;
mod unbound_retirement;
pub(crate) use unbound_retirement::UnboundCreationObservation;
#[cfg(test)]
pub(crate) use unbound_retirement::historical_v48;
#[cfg(test)]
pub(crate) use unbound_retirement::retire_unbound_pair_at;
mod uncertain_recovery;

pub(crate) use adapter::PreparedAcquiredBootstrap;
pub(crate) use cleanup::{
    NativeCleanupRuntime, PendingStorageCleanupWitness, VerifiedStorageCleanup,
};
pub use containers::{BootstrapContainerRole, runner_pair_environment};
pub(crate) use containers::{
    BootstrapPairInput, BootstrapStartReceipt, PendingBootstrapPair, StartedBootstrapPair,
    VerifiedBootstrapContainerCreated, VerifiedBootstrapMountHandoff, VerifiedCreatedStorageObject,
    VerifiedStorageNetworkCreated, VerifiedStorageNetworkRemoved, create_owned_network,
    remove_owned_network, remove_owned_network_for_jit_conflict,
    remove_owned_network_for_retirement, remove_owned_network_for_unbound_retirement,
    start_bootstrap_pair,
};
pub(crate) use delivery::ListenerDeliveryReceipt;

pub(crate) use pre_jit_recovery::{
    PreJitCleanupPhase, PreJitCleanupPhaseProof, PreJitCleanupSettlementProof,
};
pub(crate) use pre_jit_recovery::{
    recover_pre_jit_reservation, remove_bootstrap_after_terminal_failure,
};
pub(crate) use publication::JobFactsPublication;
#[cfg(test)]
pub(crate) use unbound_retirement::tests as unbound_pair_fixtures;
pub(crate) use unbound_retirement::{
    abandon_and_retire_unbound_pair_before, retire_unbound_pair, retire_uncertain_unbound_pairs,
};
pub(crate) use uncertain_recovery::{
    INITIALIZER_RECOVERY_BUDGET, UncertainRecovery, UncertainRecoveryWork, uncertain_recovery_work,
};

pub(crate) use publication::job_facts_publication;
pub(in crate::launch) use storage::StorageObservationContext;
pub(crate) use storage::VerifiedStorageResourceBinding;
pub(in crate::launch) use storage::observe_storage_resources_with_provider;
pub use storage::{ObservedContainerCgroup, observed_cgroups};
pub(crate) use storage::{ObservedStorageAttachment, ObservedStorageRole};
