//! Pure generation descriptor for the fixed cache payload namespace.
//!
//! This describes source bindings only. The authenticated runtime helper owns
//! directory capture, descriptor lifetime, clearing and materialization.

use velnor_actions_contract::CacheSnapshotDomain;

/// Generator-owned payload namespace shared by every cache domain.
pub const CACHE_PAYLOAD_ROOT_EXPR: &str = "${{ runner.temp }}/velnor";
/// Exact source-helper environment binding for the payload namespace.
pub const CACHE_PAYLOAD_ROOT_ENV: &str = "VELNOR_CACHE_PAYLOAD_ROOT";
/// Runner-provided temporary directory used by runtime capture.
pub const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";

/// Immutable generation data; never a filesystem or runtime permission.
///
/// Callers select a closed domain. They cannot supply a namespace or roots.
/// The runtime must capture the exact compiled source environment before
/// observing untrusted bytes and retain its own filesystem capability.
/// Environment consistency alone does not authenticate a caller: admission
/// must also compare the helper's full environment with the source registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheNamespaceDescriptor {
    domain: CacheSnapshotDomain,
}

impl CacheNamespaceDescriptor {
    /// Select canonical roots from the contract's sole ownership table.
    #[must_use]
    pub const fn for_domain(domain: CacheSnapshotDomain) -> Self {
        Self { domain }
    }

    /// Closed owner of the selected payload roots.
    #[must_use]
    pub const fn domain(self) -> CacheSnapshotDomain {
        self.domain
    }

    /// Exact expression compiled into authenticated helper environment.
    #[must_use]
    pub const fn root_expression(self) -> &'static str {
        CACHE_PAYLOAD_ROOT_EXPR
    }

    /// Environment pair included in source identity and registry comparison.
    #[must_use]
    pub const fn namespace_environment(self) -> (&'static str, &'static str) {
        (CACHE_PAYLOAD_ROOT_ENV, self.root_expression())
    }

    /// Runner key whose concrete value the source-owned runtime captures.
    #[must_use]
    pub const fn runner_temp_environment(self) -> &'static str {
        RUNNER_TEMP_ENV
    }

    /// Exact ordered roots; no independently copied root table.
    #[must_use]
    pub fn roots(self) -> &'static [&'static str] {
        self.domain.roots()
    }
}

#[cfg(test)]
#[path = "cache_namespace_tests.rs"]
mod tests;
