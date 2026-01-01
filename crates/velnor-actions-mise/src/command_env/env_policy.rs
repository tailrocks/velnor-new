//! Child environment policies for typed Mise and task commands.

use std::ffi::OsString;

use super::{
    CREDENTIAL_ALLOWLIST_BASELINE, CREDENTIAL_ALLOWLIST_BOOTSTRAP, ENDPOINT_ENV_KEYS,
    RUSTUP_TOOLCHAIN_ENV, is_denied_credential_key, is_stripped_ambient_key, mise_cargo_path,
    proxy_passthrough,
};

/// Which parent environment a child may see: every policy inherits
/// at most what its purpose needs, and no two purposes share the
/// same token-bearing environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvPolicy {
    /// Trusted tool download: inherits minus everything outside the
    /// download allowlist, plus the full isolation overlay. Installs
    /// never load repo config (`MISE_NO_CONFIG=1` always); explicit
    /// `tool@exact` specs are the sole version authority.
    Bootstrap,
    /// Exact-base baseline lookup via `gh`: inherits minus everything
    /// outside the baseline allowlist, plus the isolation overlay.
    Baseline,
    /// Trusted evidence validation (offline): inherits minus every
    /// credential key, plus the isolation overlay.
    Verify,
    /// Pinned MBX execution: inherits the verification environment while
    /// removing Mise's Cargo command wrappers and shims plus ambient Cargo,
    /// rustc, rustdoc, and Rustup toolchain selectors so the typed Rust
    /// selection controls those programs. A validated explicit
    /// `RUSTUP_TOOLCHAIN` addition is applied after inherited values are
    /// filtered.
    Mbx,
    /// Read-only local discovery probes: inherits minus every
    /// credential key, plus the isolation overlay.
    Discovery,
    /// Repository task execution: cleared env plus proxy passthrough,
    /// the isolation overlay, and declared inputs only.
    RepoTask,
    /// Qualified task-only config visibility with owned homes, cleared env,
    /// and proxy passthrough.
    QualifiedCheck,
    /// Read-only qualified probes: explicit environment only, no overlays.
    QualifiedProbe,
    /// Qualified acquisition: owned environment, proxy passthrough, and fixed overlay.
    QualifiedAcquisition,
}

impl EnvPolicy {
    /// Credential keys this policy keeps; every other
    /// [`super::CREDENTIAL_ENV_KEYS`] entry is stripped before spawn.
    #[must_use]
    pub const fn allowed_credentials(&self) -> &'static [&'static str] {
        match self {
            Self::Bootstrap => &CREDENTIAL_ALLOWLIST_BOOTSTRAP,
            Self::Baseline => &CREDENTIAL_ALLOWLIST_BASELINE,
            Self::Verify
            | Self::Mbx
            | Self::Discovery
            | Self::RepoTask
            | Self::QualifiedCheck
            | Self::QualifiedProbe
            | Self::QualifiedAcquisition => &[],
        }
    }

    /// Whether the child inherits the parent environment (filtered).
    /// Repository tasks and qualified policies spawn cleared.
    #[must_use]
    pub const fn inherits_parent(&self) -> bool {
        !matches!(
            self,
            Self::RepoTask
                | Self::QualifiedCheck
                | Self::QualifiedProbe
                | Self::QualifiedAcquisition
        )
    }

    /// Whether a cleared child may inherit only the ambient proxy keys.
    #[must_use]
    pub const fn allows_proxy_passthrough(&self) -> bool {
        matches!(
            self,
            Self::RepoTask | Self::QualifiedCheck | Self::QualifiedAcquisition
        )
    }

    /// Full child environment over an explicit parent snapshot.
    ///
    /// The pure contract behind the spawner: inheriting policies keep
    /// the parent minus stripped credentials, endpoints, and
    /// tofu/checkpoint ambient families; cleared tasks and acquisitions keep
    /// only proxy passthrough; and every policy appends `additions` (the
    /// isolation overlay plus validated extras) last.
    #[must_use]
    pub fn child_env(
        &self,
        parent: &[(OsString, OsString)],
        additions: &[(OsString, OsString)],
    ) -> Vec<(OsString, OsString)> {
        let mut env = Vec::with_capacity(parent.len() + additions.len());
        if self.inherits_parent() {
            let allowed = self.allowed_credentials();
            for (key, value) in parent {
                let name = key.to_string_lossy();
                let stripped = (is_denied_credential_key(&name)
                    && !allowed.contains(&name.as_ref()))
                    || ENDPOINT_ENV_KEYS.contains(&name.as_ref())
                    || is_stripped_ambient_key(&name)
                    || (matches!(self, Self::Mbx) && name == RUSTUP_TOOLCHAIN_ENV);
                if !stripped {
                    env.push((key.clone(), value.clone()));
                }
            }
        } else if self.allows_proxy_passthrough() {
            env.extend(proxy_passthrough(parent));
        }
        env.extend(additions.iter().cloned());
        if matches!(self, Self::Mbx) {
            // MBX honors `CARGO`, and Cargo honors `RUSTC`/`RUSTDOC`, as
            // executable overrides. Keeping ambient tool paths can bypass
            // the exact Rust tool selected by Mise. Resolve these programs
            // only from the sanitized, Mise-selected toolchain.
            env.retain(|(key, _)| key != "CARGO" && key != "RUSTC" && key != "RUSTDOC");
            mise_cargo_path::sanitize(&mut env, parent);
        }
        env
    }
}
