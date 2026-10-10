//! Immutable source and helper identities supplied by the release build.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompiledReleaseIdentity {
    source_sha: [u8; 20],
    helper_sha256: [u8; 32],
}

impl CompiledReleaseIdentity {
    /// Return the compiled 20-byte source commit.
    pub(crate) const fn source_sha(&self) -> &[u8; 20] {
        &self.source_sha
    }

    /// Return the compiled 32-byte helper asset digest.
    pub(crate) const fn helper_sha256(&self) -> &[u8; 32] {
        &self.helper_sha256
    }
}

include!(concat!(env!("OUT_DIR"), "/compile_identity.rs"));

/// Return the identities bound to this build, if this is a release build.
pub(crate) const fn compiled_release_identity() -> Option<CompiledReleaseIdentity> {
    COMPILED_RELEASE_IDENTITY
}

#[cfg(test)]
#[path = "compile_identity_tests.rs"]
mod tests;
