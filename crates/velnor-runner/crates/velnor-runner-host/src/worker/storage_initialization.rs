// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod adapter;
mod pending;
mod process;
mod proof;
mod ready;
pub(crate) mod request_result;
pub(crate) mod specification;
mod verification;

pub(crate) use adapter::{finish_after_pair_mounts, initialize};
pub(crate) use pending::PendingStorageInitialization;
pub(crate) use proof::{
    StorageInitializerCreateResponse, VerifiedStorageInitializerAbortRemoval,
    VerifiedStorageInitializerAbsent, VerifiedStorageInitializerAwaitingHandoff,
    VerifiedStorageInitializerCreated, VerifiedStorageInitializerRemoved,
    VerifiedStorageInitializerResult, VerifiedStorageInitializerStarted,
};
pub(crate) use ready::VerifiedStorageInitializationReady;
pub(crate) use request_result::ObservedStorageInitialization;
pub(crate) use specification::{StorageInitializationImage, StorageInitializationLimits};
pub(crate) use verification::pre_jit_container_matches;
