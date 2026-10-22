use std::path::Path;

use velnor_actions_contract::package_release::PackageReleaseReceipt;

use super::{VerifiedAttestedSignedPackage, VerifiedSignedPackage};

impl VerifiedAttestedSignedPackage {
    pub(crate) fn closure(
        &self,
    ) -> std::sync::Arc<crate::filesystem_owner::held_directory::HeldDirectory> {
        std::sync::Arc::clone(&self.closure)
    }

    pub(crate) fn plan(&self) -> &velnor_actions_contract::PackageReleasePlan {
        self.signed.plan()
    }

    pub(crate) fn receipt(&self) -> &PackageReleaseReceipt {
        self.signed.receipt()
    }

    pub(crate) fn directory(&self) -> &Path {
        self.signed.directory()
    }
}

impl VerifiedSignedPackage {
    pub(crate) fn plan(&self) -> &velnor_actions_contract::PackageReleasePlan {
        &self.plan
    }

    pub(crate) fn receipt(&self) -> &PackageReleaseReceipt {
        &self.receipt
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
}
