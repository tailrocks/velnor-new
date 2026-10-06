//! Exact, digest-bound Rustup bootstrap for supported Linux/macOS hosts.
//!
//! Prepare the owned manager before Mise's Rust backend. Its initialized
//! manager check then bypasses the floating `https://sh.rustup.rs` script.

use super::rust_proxies::{RUSTUP_SHA256_LINUX_AMD64, RUSTUP_SHA256_MACOS_ARM64, RUSTUP_VERSION};

#[path = "catalog_rust_prepare_purpose.rs"]
mod purpose;
pub use purpose::RuntimePreparationPurpose;

/// Closed Rust runtime hosts supported by generated workflow roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustHost {
    /// Default root compiler on Ubuntu x64.
    LinuxAmd64,
    /// Desktop compiler on native macOS arm64.
    MacosArm64,
}

impl RustHost {
    /// Exact Rust host ABI, independent from requested cross targets.
    #[must_use]
    pub const fn target_triple(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "x86_64-unknown-linux-gnu",
            Self::MacosArm64 => "aarch64-apple-darwin",
        }
    }

    /// Qualified manager digest for the supported host.
    #[must_use]
    pub const fn sha256(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => RUSTUP_SHA256_LINUX_AMD64,
            Self::MacosArm64 => RUSTUP_SHA256_MACOS_ARM64,
        }
    }

    /// Host-supported SHA256 verifier; command is fixed by compiled host.
    #[must_use]
    pub const fn sha256_command(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "/usr/bin/sha256sum -c -",
            Self::MacosArm64 => "/usr/bin/shasum -a 256 -c -",
        }
    }
}

/// Typed archive binding for one supported hosted-runner architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RustupBootstrap {
    /// Exact manager version, also part of tool-cache identity.
    version: &'static str,
    /// Direct official archive URL; no latest/bootstrap script resolution.
    url: &'static str,
    /// Qualified SHA256 of the installer and resulting manager executable.
    sha256: &'static str,
    host: RustHost,
}

impl RustupBootstrap {
    /// Exact Linux x64 binding used by both supported Ubuntu labels.
    #[must_use]
    pub const fn linux_amd64() -> Self {
        Self::for_host(RustHost::LinuxAmd64)
    }

    /// Closed host selects its exact manager archive and verifier together.
    #[must_use]
    pub const fn for_host(host: RustHost) -> Self {
        Self {
            version: RUSTUP_VERSION,
            url: match host {
                RustHost::LinuxAmd64 => {
                    "https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-unknown-linux-gnu/rustup-init"
                }
                RustHost::MacosArm64 => {
                    "https://static.rust-lang.org/rustup/archive/1.29.1/aarch64-apple-darwin/rustup-init"
                }
            },
            sha256: host.sha256(),
            host,
        }
    }

    /// Qualified manager version used in tool-cache identity.
    #[must_use]
    pub const fn version(self) -> &'static str {
        self.version
    }

    /// Exact official archive URL.
    #[must_use]
    pub const fn url(self) -> &'static str {
        self.url
    }

    /// Qualified installer and resulting manager SHA256.
    #[must_use]
    pub const fn sha256(self) -> &'static str {
        self.sha256
    }

    pub(in crate::catalog) const fn root_candidate_executable_roster() -> &'static [&'static str] {
        &[
            "bash",
            "/usr/bin/python3",
            "curl",
            "/usr/bin/sha256sum",
            "chmod",
            "rm",
            "mkdir",
            "ln",
            "${VELNOR_ROOT_RUST_CANDIDATE_ROOT}/rustup-bootstrap/rustup-init",
            "${CARGO_HOME}/bin/rustup",
        ]
    }

    /// Idempotent typed acquisition/install payload under owned homes.
    ///
    /// Valid complete state performs no network/install work. Missing or
    /// corrupt managers are replaced using the verified exact installer;
    /// unrelated Cargo registry/source state remains intact.
    /// # Errors
    /// Rejects a compiler host outside the closed preparation purpose.
    pub fn script(self, purpose: RuntimePreparationPurpose) -> Result<String, crate::MiseError> {
        if matches!(purpose, RuntimePreparationPurpose::RootRustCandidate(_)) {
            return Err(crate::MiseError::Contract {
                problem: "candidate_requires_acquired_native_closure_boundary".to_owned(),
            });
        }
        if !purpose.supports_host(self.host) {
            return Err(crate::MiseError::Contract {
                problem: "rustup_preparation_purpose_host_mismatch".to_owned(),
            });
        }
        Ok(format!(
            "set -eu; export RUSTUP_AUTO_INSTALL=0; \
             test ! -L \"$CARGO_HOME\"; \
             test ! -L \"$CARGO_HOME/bin\"; \
             test ! -L \"$RUSTUP_HOME\"; \
             test ! -L \"$RUSTUP_HOME/settings.toml\"; \
             test ! -L \"$RUSTUP_HOME/toolchains\"; \
             test ! -L \"$RUNNER_TEMP/{namespace}\"; \
             manager=\"$CARGO_HOME/bin/rustup\"; \
             if ! {{ test -f \"$manager\" && test ! -L \"$manager\" && test -x \"$manager\" && test -f \"$RUSTUP_HOME/settings.toml\" && test ! -L \"$RUSTUP_HOME/settings.toml\" && printf '%s  %s\\n' '{sha}' \"$manager\" | {verify}; }}; then \
               {acquire}; {initialize}; \
             fi; \
             {self_update}",
            sha = self.sha256,
            verify = self.host.sha256_command(),
            namespace = purpose.namespace_relative(),
            acquire = self.acquire_source(purpose),
            initialize = self.initialize_source(purpose),
            self_update = DISABLE_SELF_UPDATE,
        ))
    }

    pub(in crate::catalog) fn candidate_acquire_source(
        self,
        root: crate::root_rust_candidate_root::RootRustCandidateRoot,
    ) -> Result<String, crate::MiseError> {
        self.candidate_source(root, false)
    }

    pub(in crate::catalog) fn candidate_initialize_source(
        self,
        root: crate::root_rust_candidate_root::RootRustCandidateRoot,
    ) -> Result<String, crate::MiseError> {
        self.candidate_source(root, true)
    }

    fn candidate_source(
        self,
        root: crate::root_rust_candidate_root::RootRustCandidateRoot,
        initialize: bool,
    ) -> Result<String, crate::MiseError> {
        let purpose = RuntimePreparationPurpose::RootRustCandidate(root);
        if !purpose.supports_host(self.host) {
            return Err(crate::MiseError::Contract {
                problem: "rustup_preparation_purpose_host_mismatch".to_owned(),
            });
        }
        let source = if initialize {
            self.initialize_source(purpose)
        } else {
            self.acquire_source(purpose)
        };
        let suffix = if initialize { DISABLE_SELF_UPDATE } else { ":" };
        Ok(format!(
            "set -eu; export RUSTUP_AUTO_INSTALL=0; {source}; {suffix}"
        ))
    }

    fn acquire_source(self, purpose: RuntimePreparationPurpose) -> String {
        format!(
            "bootstrap=\"$RUNNER_TEMP/{namespace}/{leaf}\"; \
             test ! -L \"$bootstrap\"; \
             mkdir -p \"$bootstrap\" \"$CARGO_HOME/bin\" \"$RUSTUP_HOME\"; \
             test ! -L \"$bootstrap/rustup-init\"; \
             curl --disable --fail --silent --show-error --location '{url}' -o \"$bootstrap/rustup-init\"; \
             printf '%s  %s\\n' '{sha}' \"$bootstrap/rustup-init\" | {verify}; \
             chmod +x \"$bootstrap/rustup-init\"",
            namespace = purpose.namespace_relative(),
            leaf = purpose.bootstrap_leaf(),
            url = self.url,
            sha = self.sha256,
            verify = self.host.sha256_command(),
        )
    }

    fn initialize_source(self, purpose: RuntimePreparationPurpose) -> String {
        format!(
            "bootstrap=\"$RUNNER_TEMP/{namespace}/{leaf}\"; \
             manager=\"$CARGO_HOME/bin/rustup\"; \
             test -f \"$bootstrap/rustup-init\"; test ! -L \"$bootstrap/rustup-init\"; \
             printf '%s  %s\\n' '{sha}' \"$bootstrap/rustup-init\" | {verify}; \
             rm -f -- \"$manager\"; \
             \"$bootstrap/rustup-init\" --default-toolchain none --no-update-default-toolchain --no-modify-path --profile minimal -y; \
             printf '%s  %s\\n' '{sha}' \"$manager\" | {verify}",
            namespace = purpose.namespace_relative(),
            leaf = purpose.bootstrap_leaf(),
            sha = self.sha256,
            verify = self.host.sha256_command(),
        )
    }
}

const DISABLE_SELF_UPDATE: &str = "\"$manager\" set auto-self-update disable";

#[cfg(test)]
mod tests {
    use super::{RuntimePreparationPurpose, RustupBootstrap};

    #[test]
    fn exact_archive_and_manager_have_one_integrity_binding() -> Result<(), crate::MiseError> {
        let bootstrap = RustupBootstrap::linux_amd64();
        assert!(
            bootstrap
                .url
                .contains(&format!("/archive/{}/", bootstrap.version))
        );
        assert_eq!(bootstrap.sha256.len(), 64);
        assert!(
            bootstrap
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        let script = bootstrap.script(RuntimePreparationPurpose::Full)?;
        assert!(script.starts_with("set -eu; export RUSTUP_AUTO_INSTALL=0;"));
        assert!(script.contains("curl --disable --fail --silent --show-error --location"));
        assert!(!script.contains('\n'));
        assert_eq!(script.matches(bootstrap.sha256).count(), 4);
        assert!(script.contains("--default-toolchain none --no-update-default-toolchain --no-modify-path --profile minimal -y"));
        assert!(!script.contains("sh.rustup.rs"));
        Ok(())
    }

    #[test]
    fn source_intent_bootstrap_has_its_own_namespace_and_root_host() -> Result<(), crate::MiseError>
    {
        let root = crate::source_intent_cold_root::SourceIntentColdRoot::root_linux();
        let purpose = RuntimePreparationPurpose::SourceIntent(root);
        let script = RustupBootstrap::for_host(root.host()).script(purpose)?;
        assert!(script.contains("$RUNNER_TEMP/velnor-control/source-intent/rustup-bootstrap"));
        assert!(!script.contains("$RUNNER_TEMP/velnor/rustup-bootstrap"));
        assert!(
            RustupBootstrap::for_host(super::RustHost::MacosArm64)
                .script(purpose)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn candidate_cannot_use_combined_download_and_execution() {
        let root = crate::root_rust_candidate_root::RootRustCandidateRoot::root_linux();
        assert!(
            RustupBootstrap::linux_amd64()
                .script(RuntimePreparationPurpose::RootRustCandidate(root))
                .is_err()
        );
    }
}
