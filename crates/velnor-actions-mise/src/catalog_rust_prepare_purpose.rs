//! Closed namespaces for compiler preparation; no caller-provided paths.

use crate::root_rust_candidate_root::{RootRustCandidateLeaf, RootRustCandidateRoot};
use crate::source_intent_cold_root::{SourceIntentColdLeaf, SourceIntentColdRoot};

use super::RustHost;

/// Owner-selected purpose, distinct from tool cache domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePreparationPurpose {
    /// Existing Full tooling preparation for supported compiler roles.
    Full,
    /// Fresh `RootLinux` compiler under the independent `SourceIntent` control root.
    SourceIntent(SourceIntentColdRoot),
    /// Source-only compiler candidate; never an installed SDK or cache grant.
    RootRustCandidate(RootRustCandidateRoot),
}

impl RuntimePreparationPurpose {
    pub(super) const fn namespace_relative(self) -> &'static str {
        match self {
            Self::Full => "velnor",
            Self::SourceIntent(root) => root.relative_to_runner_temp(),
            Self::RootRustCandidate(root) => root.relative_to_runner_temp(),
        }
    }

    pub(super) const fn bootstrap_leaf(self) -> &'static str {
        match self {
            Self::Full => "rustup-bootstrap",
            Self::SourceIntent(_) => SourceIntentColdLeaf::RustupBootstrap.relative(),
            Self::RootRustCandidate(_) => RootRustCandidateLeaf::RustupBootstrap.relative(),
        }
    }

    pub(super) const fn supports_host(self, host: RustHost) -> bool {
        match self {
            Self::Full => true,
            Self::SourceIntent(root) => matches!(
                (root.host(), host),
                (RustHost::LinuxAmd64, RustHost::LinuxAmd64)
            ),
            Self::RootRustCandidate(root) => matches!(
                (root.host(), host),
                (RustHost::LinuxAmd64, RustHost::LinuxAmd64)
            ),
        }
    }
}
