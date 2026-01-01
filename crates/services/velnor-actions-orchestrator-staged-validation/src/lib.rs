//! Staged-tree writing plus staged validation before any write.
//!
//! Rendered trees materialize into isolated staging ([`write`]) and
//! validate there ([`validate`]: actionlint, shellcheck via
//! [`validate_shell`], zizmor via [`validate_zizmor`], plus the
//! Velnor-repository bootstrap files) before generate replaces,
//! previews, or publishes anything.

pub mod validate;
pub mod validate_shell;
pub mod validate_zizmor;
pub mod write;
