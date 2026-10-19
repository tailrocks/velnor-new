//! Publish one trusted source-proof record at a fixed control path.

mod wire;

#[cfg(target_os = "linux")]
mod publish;
pub use wire::{
    ExecutionPublicationContext, ExecutionPublicationFile, ExecutionPublicationKind,
    ExecutionPublicationRecord,
};

/// Maximum input bytes for one publication request.
pub const PUBLICATION_REQUEST_LIMIT: usize = 20_480;

/// Publish one challenge or decision record in the fixed control directory.
///
/// # Errors
///
/// Returns an error if the request, publisher mount, or control file is not safe.
#[cfg(target_os = "linux")]
pub fn publish_execution_control(
    context: ExecutionPublicationContext,
) -> Result<ExecutionPublicationRecord, crate::ProbeError> {
    publish::publish_execution_control(context)
}

/// Return an error when the selected guest does not run Linux.
///
/// # Errors
///
/// Returns an error when the selected guest does not run Linux.
#[cfg(not(target_os = "linux"))]
pub fn publish_execution_control(
    _context: ExecutionPublicationContext,
) -> Result<ExecutionPublicationRecord, crate::ProbeError> {
    Err(crate::ProbeError::Invalid("linux_guest_required"))
}
