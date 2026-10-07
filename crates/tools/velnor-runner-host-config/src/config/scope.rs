use serde::Deserialize;

/// Registration scope supported by this product configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationScopeKind {
    /// Repository-scoped GitHub App/PAT registration.
    Repository,
    /// Organization-scoped GitHub App/PAT registration.
    Organization,
}

/// Exact credential-minting scope carried with a Scale Set binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationScope {
    /// Repository-scoped registration, distinct from the target trust repo.
    Repository {
        /// Exact repository owner.
        owner: String,
        /// Exact repository name.
        repository: String,
    },
    /// Organization-scoped registration.
    Organization {
        /// Exact organization login.
        organization: String,
    },
}
