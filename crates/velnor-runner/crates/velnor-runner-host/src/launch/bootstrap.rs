// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) mod adapter;
mod cleanup;
mod containers;
mod delivery;
mod pre_jit_recovery;
mod publication;
mod storage;

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
    remove_owned_network_for_retirement, start_bootstrap_pair,
};
pub(crate) use delivery::ListenerDeliveryReceipt;

pub(crate) use pre_jit_recovery::{
    PreJitCleanupPhase, PreJitCleanupPhaseProof, PreJitCleanupSettlementProof,
};
pub(crate) use pre_jit_recovery::{
    recover_pre_jit_reservation, remove_bootstrap_after_terminal_failure,
};
pub(crate) use publication::JobFactsPublication;

pub(crate) use publication::job_facts_publication;
pub(in crate::launch) use storage::StorageObservationContext;
pub(crate) use storage::VerifiedStorageResourceBinding;
pub(in crate::launch) use storage::observe_storage_resources_with_provider;
pub(crate) use storage::{ObservedContainerCgroup, ObservedStorageAttachment, ObservedStorageRole};
