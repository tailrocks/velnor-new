//! Numeric signed preview version ordering, shared with every history link.

use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::{
    package_release::PackageReleaseReceipt, preview_identity::PreviewIdentity,
};

#[derive(Clone)]
pub(super) struct Version {
    base: [u64; 3],
    count: u64,
}

impl Version {
    pub(super) fn from_receipt(receipt: &PackageReleaseReceipt) -> Result<Self, OrchestratorError> {
        let count = receipt
            .version
            .commit_count
            .ok_or_else(|| internal("rolling_signed_version_count"))?;
        let derived =
            PreviewIdentity::derive(&receipt.version.base, &receipt.source.commit_sha, count)
                .map_err(|_| internal("rolling_signed_preview_version"))?;
        if derived.version != receipt.version.value || derived.tag != receipt.tag {
            return Err(internal("rolling_signed_version_identity"));
        }
        Self::parse(&receipt.version.base, count)
    }

    fn parse(base: &str, count: u64) -> Result<Self, OrchestratorError> {
        let components = base
            .split('.')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| internal("rolling_signed_numeric_base"))?;
        let base = components
            .try_into()
            .map_err(|_| internal("rolling_signed_numeric_base"))?;
        Ok(Self { base, count })
    }

    pub(super) fn supersedes(&self, previous: &Self) -> bool {
        self.base > previous.base || (self.base == previous.base && self.count > previous.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descendant_count_cannot_mask_base_downgrade() {
        let old = Version::parse("1.10.0", 100).expect("old");
        assert!(
            !Version::parse("1.9.99", 10_000)
                .expect("lower base")
                .supersedes(&old)
        );
        assert!(
            !Version::parse("1.10.0", 100)
                .expect("same identity count")
                .supersedes(&old)
        );
        assert!(
            !Version::parse("1.10.0", 99)
                .expect("lower count")
                .supersedes(&old)
        );
        assert!(
            Version::parse("1.10.0", 101)
                .expect("next count")
                .supersedes(&old)
        );
        assert!(
            Version::parse("1.11.0", 101)
                .expect("higher base")
                .supersedes(&old)
        );
    }
}
