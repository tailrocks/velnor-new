//! Pure source identity and closed strict-online Homebrew command policy.

use super::targets::SourceTargets;

/// Preserve the historical repository-root Homebrew watch boundary.
#[must_use]
pub fn input_paths(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|path| {
            path.as_str() == "Brewfile"
                || path.starts_with("Formula/")
                || path.starts_with("Casks/")
        })
        .cloned()
        .collect()
}
use velnor_actions_contract::ContractError;

/// Reviewed tool owner supplies the exact source and portable-Ruby identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrewSourceIdentity {
    version: String,
    sha: String,
    ruby: String,
    x86_ruby_sha256: String,
    arm_ruby_sha256: String,
}

impl BrewSourceIdentity {
    /// Bind exact reviewed source and Ruby release hashes, never an ambient Brew.
    /// # Errors
    /// Rejects malformed release identities or immutable source hashes.
    pub fn reviewed(
        version: &str,
        sha: &str,
        ruby: &str,
        x86: &str,
        arm: &str,
    ) -> Result<Self, ContractError> {
        let release = |value: &str| {
            value.split('.').count() == 3
                && value
                    .split('.')
                    .all(|part| !part.is_empty() && part.bytes().all(|ch| ch.is_ascii_digit()))
        };
        let digest = |value: &str, length| {
            value.len() == length
                && value
                    .bytes()
                    .all(|ch| ch.is_ascii_digit() || matches!(ch, b'a'..=b'f'))
        };
        if !release(version)
            || !release(ruby)
            || !digest(sha, 40)
            || !digest(x86, 64)
            || !digest(arm, 64)
        {
            return Err(failure("homebrew_source_identity_invalid"));
        }
        Ok(Self {
            version: version.to_owned(),
            sha: sha.to_owned(),
            ruby: ruby.to_owned(),
            x86_ruby_sha256: x86.to_owned(),
            arm_ruby_sha256: arm.to_owned(),
        })
    }

    /// Exact source release version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Exact immutable source commit.
    #[must_use]
    pub fn sha(&self) -> &str {
        &self.sha
    }

    /// Complete toolchain and audit-target semantics identity.
    #[must_use]
    pub fn recipe(&self) -> String {
        format!(
            "homebrew={}\nsource={}\nportable-ruby={}\nx86_64-linux={}\narm64-linux={}\naudit-targets=indexed-regular-source-v1;bare+explicit-scoped;skip-style",
            self.version, self.sha, self.ruby, self.x86_ruby_sha256, self.arm_ruby_sha256
        )
    }
}

/// Generation-time repository provenance determines this safe tap identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapIdentity {
    owner: String,
    name: String,
}

impl TapIdentity {
    /// Derive a bounded local tap alias from an already anchored repository slug.
    /// # Errors
    /// Rejects non-tap repositories, unsafe path components and options.
    pub fn from_repository(repository: &str) -> Result<Self, ContractError> {
        let (owner, repository) = repository
            .split_once('/')
            .ok_or_else(|| failure("homebrew_repository_invalid"))?;
        let repository = repository.to_ascii_lowercase();
        let name = repository
            .strip_prefix("homebrew-")
            .ok_or_else(|| failure("homebrew_repository_invalid"))?;
        let safe = |value: &str| {
            !value.is_empty()
                && value.starts_with(|ch: char| ch.is_ascii_alphanumeric())
                && value.ends_with(|ch: char| ch.is_ascii_alphanumeric())
                && value
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_'))
        };
        if owner.len() > 39 || name.len() > 100 || !safe(owner) || !safe(name) {
            return Err(failure("homebrew_repository_invalid"));
        }
        Ok(Self {
            owner: owner.to_ascii_lowercase(),
            name: name.to_owned(),
        })
    }

    /// Source owner component.
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }
    /// Source tap component.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Fully qualified source tap alias.
    #[must_use]
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// Preserve the exact prior audit and close tolerant-import false greens.
#[must_use]
pub fn audit_arguments(targets: &SourceTargets) -> Vec<(&'static str, Vec<String>)> {
    let base = vec![
        "audit".to_owned(),
        "--strict".to_owned(),
        "--online".to_owned(),
    ];
    let mut phases = vec![("homebrew-audit", base.clone())];
    for (phase, scope, refs) in [
        ("homebrew-audit-formula", "--formula", targets.formulae()),
        ("homebrew-audit-cask", "--cask", targets.casks()),
    ] {
        if !refs.is_empty() {
            let mut argv = base.clone();
            argv.extend([scope.to_owned(), "--skip-style".to_owned()]);
            argv.extend_from_slice(refs);
            phases.push((phase, argv));
        }
    }
    phases
}

pub(super) fn failure(reason: &str) -> ContractError {
    ContractError::identity("homebrew", reason)
}
