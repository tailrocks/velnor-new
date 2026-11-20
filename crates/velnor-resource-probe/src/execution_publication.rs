// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod wire;

#[cfg(target_os = "linux")]
mod publish;
pub use wire::{
    ExecutionPublicationContext, ExecutionPublicationFile, ExecutionPublicationKind,
    ExecutionPublicationRecord,
};

pub const PUBLICATION_REQUEST_LIMIT: usize = 20_480;

#[cfg(target_os = "linux")]
pub fn publish_execution_control(
    context: ExecutionPublicationContext,
) -> Result<ExecutionPublicationRecord, crate::ProbeError> {
    publish::publish_execution_control(context)
}

#[cfg(not(target_os = "linux"))]
pub fn publish_execution_control(
    _context: ExecutionPublicationContext,
) -> Result<ExecutionPublicationRecord, crate::ProbeError> {
    Err(crate::ProbeError::Invalid("linux_guest_required"))
}
