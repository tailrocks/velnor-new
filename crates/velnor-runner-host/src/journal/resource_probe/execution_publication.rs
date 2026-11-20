// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const MAX_PARENT_TEXT: usize = 256;
const MAX_RECOVERY_PROJECTION_BYTES: usize = 32 * 1024;

mod context;
mod journal;
mod parent;
mod types;
mod validation;

pub(crate) use types::{
    ExecutionPublicationContext, ExecutionPublicationParent, ExecutionPublicationRecoveryRecord,
    ExecutionPublicationSeed, PublicationKind,
};
