#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompiledReleaseIdentity {
    source_sha: [u8; 20],
    helper_sha256: [u8; 32],
}

impl CompiledReleaseIdentity {
    pub(crate) const fn source_sha(&self) -> &[u8; 20] {
        &self.source_sha
    }

    pub(crate) const fn helper_sha256(&self) -> &[u8; 32] {
        &self.helper_sha256
    }
}

include!(concat!(env!("OUT_DIR"), "/compile_identity.rs"));

pub(crate) const fn compiled_release_identity() -> Option<CompiledReleaseIdentity> {
    COMPILED_RELEASE_IDENTITY
}
