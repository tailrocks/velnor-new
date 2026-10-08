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

/// Organization-scoped registration intent tied to one exact private target
/// repository. The repository and organization must have the same owner name;
/// this prevents using one repository's trust configuration to bootstrap a
/// different organization scope.
#[must_use]
pub struct OrganizationAdminEvidence {
    organization: String,
    target_repository_id: i64,
    target_repository_full_name: String,
}

impl OrganizationAdminEvidence {
    pub(in crate::registration) fn from_repository(
        repository: RepositoryAdminEvidence,
        organization: &str,
    ) -> Result<Self, crate::SessionError> {
        let (owner, _) = repository
            .full_name
            .split_once('/')
            .ok_or(crate::WireError::Malformed)?;
        if !safe_scope_name(organization) || !owner.eq_ignore_ascii_case(organization) {
            return Err(crate::WireError::RegistrationRejected.into());
        }
        Ok(Self {
            organization: organization.to_owned(),
            target_repository_id: repository.id,
            target_repository_full_name: repository.full_name,
        })
    }

    /// Exact organization login used for registration scope and config URL.
    #[must_use]
    pub fn organization(&self) -> &str {
        &self.organization
    }

    /// Immutable target repository ID retained separately from registration scope.
    #[must_use]
    pub const fn target_repository_id(&self) -> i64 {
        self.target_repository_id
    }

    /// Canonical target repository full name.
    #[must_use]
    pub fn target_repository_full_name(&self) -> &str {
        &self.target_repository_full_name
    }
}

impl fmt::Debug for OrganizationAdminEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OrganizationAdminEvidence")
            .field("organization", &self.organization)
            .field("target_repository_id", &self.target_repository_id)
            .field(
                "target_repository_full_name",
                &self.target_repository_full_name,
            )
            .finish()
    }
}

/// Organization-scoped registration token for one bounded metadata exchange.
#[must_use]
pub struct OrganizationDiscoveryToken {
    organization: String,
    target_repository_id: i64,
    target_repository_full_name: String,
    config_url: String,
    registration_token: RegistrationToken,
}

impl OrganizationDiscoveryToken {
    pub(in crate::registration) fn new(
        organization: String,
        target_repository_id: i64,
        target_repository_full_name: String,
        registration_token: RegistrationToken,
    ) -> Self {
        let config_url = format!("https://github.com/{organization}");
        Self {
            organization,
            target_repository_id,
            target_repository_full_name,
            config_url,
            registration_token,
        }
    }

    pub(in crate::registration) fn organization(&self) -> &str {
        &self.organization
    }

    pub(in crate::registration) const fn target_repository_id(&self) -> i64 {
        self.target_repository_id
    }

    pub(in crate::registration) fn target_repository_full_name(&self) -> &str {
        &self.target_repository_full_name
    }

    pub(in crate::registration) fn config_url(&self) -> &str {
        &self.config_url
    }

    pub(in crate::registration) fn registration_token(&self) -> &str {
        self.registration_token.expose()
    }
}

impl fmt::Debug for OrganizationDiscoveryToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OrganizationDiscoveryToken([redacted])")
    }
}

impl Drop for OrganizationDiscoveryToken {
    fn drop(&mut self) {
        self.organization.zeroize();
        self.target_repository_full_name.zeroize();
        self.config_url.zeroize();
    }
}

fn safe_scope_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
