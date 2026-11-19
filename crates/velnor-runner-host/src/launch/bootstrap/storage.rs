// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod attachment;
mod backing;
mod binding;
pub(crate) use attachment::ObservedStorageAttachment;
pub(crate) use backing::VerifiedRootBackingMapping;
pub use binding::ObservedContainerCgroup;
pub(crate) use binding::{ObservedStorageRole, VerifiedStorageResourceBinding};

use crate::journal::{StorageObserver, StorageRole};

mod observation;
mod production;
mod validation;

pub(crate) use production::{PreparedTmpfsStorage, create_and_initialize};

pub use observation::observed_cgroups;
pub(in crate::launch) use observation::{
    StorageObservationContext, observe_storage_resources_with_provider,
};
