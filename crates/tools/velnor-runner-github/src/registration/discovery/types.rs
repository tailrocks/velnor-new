//! Secret-bearing and repository-scoped values used by discovery.

use std::fmt;

use zeroize::Zeroize;

use crate::RegistrationToken;

/// Repository metadata proving the exact private repository and the current
/// caller's repository administrator permission for discovery bootstrap.
///
/// This value is deliberately narrower than a pool or job trust permit. It is
/// neither serializable nor clonable and can be consumed by one registration
/// token request. The caller must still establish credential provenance and
/// persist side-effect intent in the host before the request.
#[must_use]
pub struct RepositoryAdminEvidence {
    pub(super) id: i64,
    pub(super) owner: String,
    pub(super) repository: String,
    pub(super) full_name: String,
}

impl RepositoryAdminEvidence {
    pub(in crate::registration) fn from_repository_metadata(
        id: i64,
        owner: String,
        repository: String,
        full_name: String,
    ) -> Self {
        Self {
            id,
            owner,
            repository,
            full_name,
        }
    }

    /// Positive immutable GitHub repository id from the metadata response.
    #[must_use]
    pub const fn repository_id(&self) -> i64 {
        self.id
    }

    /// Canonical owner and repository returned by GitHub.
    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.full_name
    }

    pub(in crate::registration) fn owner(&self) -> &str {
        &self.owner
    }

    pub(in crate::registration) fn repository(&self) -> &str {
        &self.repository
    }
}

impl fmt::Debug for RepositoryAdminEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryAdminEvidence")
            .field("repository_id", &self.id)
            .field("full_name", &self.full_name)
            .field("private", &true)
            .field("admin", &true)
            .finish_non_exhaustive()
    }
}

/// A repository-scoped registration token paired with its canonical GitHub
/// config URL. The token is redacted and zeroized when dropped.
#[must_use]
pub struct RepositoryDiscoveryToken {
    pub(super) repository_id: i64,
    pub(super) repository_full_name: String,
    pub(super) config_url: String,
    pub(super) registration_token: RegistrationToken,
}

impl RepositoryDiscoveryToken {
    pub(in crate::registration) fn new(
        repository_id: i64,
        repository_full_name: String,
        config_url: String,
        registration_token: RegistrationToken,
    ) -> Self {
        Self {
            repository_id,
            repository_full_name,
            config_url,
            registration_token,
        }
    }

    pub(in crate::registration) const fn repository_id(&self) -> i64 {
        self.repository_id
    }

    pub(in crate::registration) fn repository_full_name(&self) -> &str {
        &self.repository_full_name
    }

    pub(in crate::registration) fn config_url(&self) -> &str {
        &self.config_url
    }

    pub(in crate::registration) fn registration_token(&self) -> &str {
        self.registration_token.expose()
    }
}

impl fmt::Debug for RepositoryDiscoveryToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RepositoryDiscoveryToken([redacted])")
    }
}

impl Drop for RepositoryDiscoveryToken {
    fn drop(&mut self) {
        self.repository_full_name.zeroize();
        self.config_url.zeroize();
    }
}
