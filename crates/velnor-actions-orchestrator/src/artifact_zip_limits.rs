use crate::package_release_artifacts::{
    MAX_FILE_COUNT, MAX_HANDOFF_BYTES, MAX_PAYLOAD_BYTES, MAX_SBOM_BYTES, MAX_TOTAL_BYTES,
};
use velnor_actions_contract::{
    MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES, MAX_GITHUB_EVIDENCE_BUNDLE_BYTES,
};

pub(crate) const NATIVE_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum ZipProfile {
    Recovery,
    PreviewLeaf,
    PreviewAggregate,
    Evidence,
}

impl ZipProfile {
    pub(crate) const fn unpacked(self) -> u64 {
        match self {
            Self::Recovery => 4 * 1024 * 1024 * 1024,
            Self::PreviewLeaf => MAX_PAYLOAD_BYTES + MAX_SBOM_BYTES + NATIVE_MANIFEST_BYTES,
            Self::PreviewAggregate => MAX_TOTAL_BYTES + MAX_HANDOFF_BYTES + NATIVE_MANIFEST_BYTES,
            Self::Evidence => MAX_GITHUB_EVIDENCE_BUNDLE_BYTES,
        }
    }

    pub(crate) const fn entries(self) -> usize {
        match self {
            Self::Recovery | Self::Evidence => super::MAX_ENTRIES,
            Self::PreviewLeaf => 3,
            Self::PreviewAggregate => MAX_FILE_COUNT + 1,
        }
    }

    pub(crate) const fn member_unpacked(self) -> u64 {
        match self {
            Self::Evidence => MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES,
            Self::Recovery | Self::PreviewLeaf | Self::PreviewAggregate => self.unpacked(),
        }
    }

    pub(crate) const fn compressed(self) -> u64 {
        match self {
            Self::Recovery => 1024 * 1024 * 1024,
            Self::PreviewLeaf => compressed_bound(self.unpacked(), 3),
            Self::PreviewAggregate => compressed_bound(self.unpacked(), self.entries() as u64),
            Self::Evidence => MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES,
        }
    }
}

pub(crate) const fn compressed_bound(raw: u64, entries: u64) -> u64 {
    const ENTRY_FRAMING: u64 = 5 * 65_535 + 256;
    const END_FRAMING: u64 = 65_535 + 256;
    raw + raw.div_ceil(8) + entries * ENTRY_FRAMING + END_FRAMING
}

pub(crate) fn leaf_union_compressed_bound(pairs: usize) -> u64 {
    compressed_bound(MAX_TOTAL_BYTES + NATIVE_MANIFEST_BYTES, pairs as u64 * 3)
}
