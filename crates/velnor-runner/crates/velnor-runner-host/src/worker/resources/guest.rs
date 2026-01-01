//! Typed guest resource values shared with the host capacity policy.
//!
//! The launch path does not yet wire samples into capacity policy; only the
//! value types below remain for the capacity-policy consumer. The unreconciled
//! probe/sampler scaffolding was never wired and has been removed.

/// Timestamped cached values consumed by the host capacity policy.
pub(crate) mod sampler;

/// The guest resources needed by the resource policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GuestResourceSample {
    /// Guest CPU capacity in millicores, derived from the daemon's NCPU field.
    pub(crate) cpu_millicores: Option<u32>,
    /// Guest `MemAvailable`, converted from KiB to bytes.
    pub(crate) memory_available_bytes: Option<u64>,
    /// Memory PSI `some avg10`, in hundredths of a percentage point.
    ///
    /// For example, `125` means `1.25%`; `10_000` means `100%`.
    pub(crate) memory_psi_some_avg10_bps: Option<u16>,
    /// Available bytes on the filesystem containing the actual Docker root.
    pub(crate) docker_root_free_bytes: Option<u64>,
}
