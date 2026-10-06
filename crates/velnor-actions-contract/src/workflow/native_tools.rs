//! Generation-time native execution authority; never deserialized from policy.

use std::collections::BTreeMap;

use crate::ContractError;

/// Closed credential admission for a compiled trusted operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCredentialScope {
    /// Repository compilation and dependency hydration see no credentials.
    Anonymous,
    /// Read-only fixed GitHub release inspection keeps only the GitHub token.
    GithubReadOnly,
    /// Fixed protected failure observer receives only its issue-write GitHub token.
    GithubIssueWrite,
    /// Fixed crates.io trusted publication receives only the runner OIDC pair.
    RustRegistryPublishOidc,
    /// Fixed initial crates.io publication receives only its bootstrap token.
    RustRegistryPublishBootstrap,
    /// Fixed GitHub release publication receives only the GitHub token.
    GithubReleasePublish,
    /// Fixed signing/notarization sees only the explicit Apple credential keys.
    AppleSigning,
    /// Fixed APT repository signing keeps only the two GPG credential bindings.
    AptSigning,
    /// Fixed OCI registry publication uses the isolated login configuration.
    OciRegistryPublish,
}

impl NativeCredentialScope {
    /// Credential keys this operation alone may receive.
    #[must_use]
    pub const fn allowed_keys(self) -> &'static [&'static str] {
        match self {
            Self::Anonymous => &[],
            Self::GithubReadOnly | Self::GithubIssueWrite | Self::GithubReleasePublish => {
                &["GH_TOKEN"]
            }
            Self::RustRegistryPublishOidc => &[
                "ACTIONS_ID_TOKEN_REQUEST_URL",
                "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            ],
            Self::RustRegistryPublishBootstrap => &["CARGO_REGISTRY_TOKEN"],
            Self::AppleSigning => &[
                "EXPECTED_TEAM_ID",
                "EXPECTED_CERT_SHA256",
                "DEVELOPER_ID_APPLICATION",
                "DEVELOPER_ID_APPLICATION_P12_BASE64",
                "DEVELOPER_ID_APPLICATION_P12_PASSWORD",
                "APP_STORE_CONNECT_API_KEY_P8",
                "APP_STORE_CONNECT_API_KEY_PATH",
                "APP_STORE_CONNECT_KEY_ID",
                "APP_STORE_CONNECT_ISSUER_ID",
            ],
            Self::AptSigning => &["APT_GPG_PRIVATE_KEY", "APT_GPG_PASSPHRASE"],
            Self::OciRegistryPublish => &["DOCKER_CONFIG", "GH_TOKEN"],
        }
    }
}

/// Checked SDK execution envelope. Exact semantic admission belongs to its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledNativeExecRecipe {
    prefix: Vec<String>,
    environment: BTreeMap<String, String>,
    installed_selectors: Vec<String>,
    credential_scope: NativeCredentialScope,
    homebrew_foundation: bool,
}

impl CompiledNativeExecRecipe {
    /// Register an SDK-built envelope after checking its closed tool semantics.
    ///
    /// This structural constructor grants no catalog authority by itself.
    /// Consumers require exact reconstruction by the compiled SDK owner.
    /// # Errors
    /// Rejects malformed argv, selector inventory, or environment.
    pub fn compiled(
        prefix: Vec<String>,
        environment: BTreeMap<String, String>,
        installed_selectors: Vec<String>,
    ) -> Result<Self, ContractError> {
        Self::compiled_for_scope(
            prefix,
            environment,
            installed_selectors,
            NativeCredentialScope::Anonymous,
        )
    }

    /// Register a closed trusted operation's envelope after SDK semantic admission.
    /// # Errors
    /// Rejects malformed structural bindings; the SDK still reconstructs exact authority.
    pub fn compiled_for_scope(
        prefix: Vec<String>,
        environment: BTreeMap<String, String>,
        installed_selectors: Vec<String>,
        credential_scope: NativeCredentialScope,
    ) -> Result<Self, ContractError> {
        let value = Self {
            prefix,
            environment,
            installed_selectors,
            credential_scope,
            homebrew_foundation: false,
        };
        value.validate()?;
        Ok(value)
    }

    /// Register a closed anonymous Homebrew foundation envelope with no Mise tools.
    /// # Errors
    /// Rejects any prefix outside the cleared host execution shape.
    pub fn compiled_homebrew_foundation(
        prefix: Vec<String>,
        environment: BTreeMap<String, String>,
    ) -> Result<Self, ContractError> {
        let value = Self {
            prefix,
            environment,
            installed_selectors: Vec::new(),
            credential_scope: NativeCredentialScope::Anonymous,
            homebrew_foundation: true,
        };
        value.validate()?;
        Ok(value)
    }

    /// Structural shape only; the SDK separately checks exact catalog equality.
    /// # Errors
    /// Rejects empty, duplicate or control-bearing values and mismatched inventory.
    pub fn validate(&self) -> Result<(), ContractError> {
        let invalid = || ContractError::identity("native_exec", "invalid_compiled_recipe");
        if self.prefix.len() > 2048
            || self.installed_selectors.is_empty() && !self.homebrew_foundation
            || self.prefix.last().map(String::as_str) != Some("--")
            || self
                .prefix
                .iter()
                .chain(&self.installed_selectors)
                .any(|arg| arg.is_empty() || arg.chars().any(char::is_control))
            || self.environment.iter().any(|(key, value)| {
                key.is_empty()
                    || !key.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    })
                    || value.chars().any(char::is_control)
            })
        {
            return Err(invalid());
        }
        if self.homebrew_foundation {
            if self.credential_scope != NativeCredentialScope::Anonymous
                || !self.installed_selectors.is_empty()
                || self.prefix.get(..2) != Some(&["/usr/bin/env".to_owned(), "-i".to_owned()])
                || !self
                    .prefix
                    .ends_with(&["/usr/bin/env".to_owned(), "--".to_owned()])
                || self
                    .prefix
                    .get(2..self.prefix.len().saturating_sub(2))
                    .is_none_or(|values| values.iter().any(|value| !value.contains('=')))
            {
                return Err(invalid());
            }
            return Ok(());
        }
        let end = self.prefix.len().saturating_sub(1);
        let start = end
            .checked_sub(self.installed_selectors.len())
            .ok_or_else(invalid)?;
        if self.prefix.get(start..end) != Some(self.installed_selectors.as_slice())
            || self
                .installed_selectors
                .iter()
                .enumerate()
                .any(|(index, selector)| self.installed_selectors[..index].contains(selector))
        {
            return Err(invalid());
        }
        Ok(())
    }

    /// Complete owner-checked execution prefix, ending with the tool separator.
    #[must_use]
    pub fn prefix(&self) -> &[String] {
        &self.prefix
    }

    /// Exact owner environment; callers may not replace its control keys.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    /// Installed selector footprint bound to this execution envelope.
    #[must_use]
    pub fn installed_selectors(&self) -> &[String] {
        &self.installed_selectors
    }

    /// Exact credential purpose, bound by the compiled source record and SDK recipe.
    #[must_use]
    pub const fn credential_scope(&self) -> NativeCredentialScope {
        self.credential_scope
    }

    /// Whether this is the explicitly closed anonymous host foundation envelope.
    #[must_use]
    pub const fn is_homebrew_foundation(&self) -> bool {
        self.homebrew_foundation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix(selectors: &[&str]) -> Vec<String> {
        [
            "env",
            "-i",
            "/owned/mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
        ]
        .into_iter()
        .chain(selectors.iter().copied())
        .chain(["--"])
        .map(str::to_owned)
        .collect()
    }

    #[test]
    fn prefix_inventory_cannot_diverge_or_repeat() {
        assert!(
            CompiledNativeExecRecipe::compiled(
                prefix(&["python@3.14.0"]),
                BTreeMap::new(),
                vec!["gh@2.102.0".to_owned()],
            )
            .is_err()
        );
        assert!(
            CompiledNativeExecRecipe::compiled(
                prefix(&["python@3.14.0", "python@3.14.0"]),
                BTreeMap::new(),
                vec!["python@3.14.0".to_owned(); 2],
            )
            .is_err()
        );
    }

    #[test]
    fn source_controls_cannot_enter_recipe_wire_values() {
        let selectors = vec!["python@3.14.0".to_owned()];
        for value in ["bad\nvalue", "bad\0value"] {
            assert!(
                CompiledNativeExecRecipe::compiled(
                    prefix(&["python@3.14.0"]),
                    BTreeMap::from([("HOME".to_owned(), value.to_owned())]),
                    selectors.clone(),
                )
                .is_err()
            );
        }
    }
}
