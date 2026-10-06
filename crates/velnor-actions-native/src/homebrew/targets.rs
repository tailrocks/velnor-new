//! Pure indexed target admission; orchestration proves regular source files.

use super::audit::{TapIdentity, failure};

/// Ruby source files whose physical admission orchestration must prove.
#[must_use]
pub fn is_target_source(path: &str) -> bool {
    (path.starts_with("Formula/") || path.starts_with("Casks/")) && has_ruby_extension(path)
}

/// Homebrew inventories use the literal lowercase `.rb` suffix.
fn has_ruby_extension(path: &str) -> bool {
    path.as_bytes().ends_with(b".rb")
}
use std::collections::BTreeSet;
use velnor_actions_contract::ContractError;

/// Deterministic, scoped references for the exact admitted source inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTargets {
    formulae: Vec<String>,
    casks: Vec<String>,
}

impl SourceTargets {
    /// Admit every indexed Formula/Cask Ruby source without tolerant imports.
    /// # Errors
    /// Rejects unsafe names, duplicate scope names, or zero audit targets.
    pub fn from_inventory(files: &[String], tap: &TapIdentity) -> Result<Self, ContractError> {
        let mut targets = Self {
            formulae: Vec::new(),
            casks: Vec::new(),
        };
        let mut formula_names = BTreeSet::new();
        let mut cask_names = BTreeSet::new();
        for path in files {
            let selected = if path.starts_with("Formula/") && has_ruby_extension(path) {
                Some((&mut targets.formulae, &mut formula_names))
            } else if path.starts_with("Casks/") && has_ruby_extension(path) {
                Some((&mut targets.casks, &mut cask_names))
            } else {
                None
            };
            let Some((refs, names)) = selected else {
                continue;
            };
            let name = token(path)?;
            if !names.insert(name.to_owned()) {
                return Err(failure("homebrew_duplicate_source_target"));
            }
            refs.push(format!("{}/{name}", tap.slug()));
        }
        if targets.formulae.is_empty() && targets.casks.is_empty() {
            return Err(failure("homebrew_audit_targets_missing"));
        }
        targets.formulae.sort();
        targets.casks.sort();
        Ok(targets)
    }

    /// Exact source-qualified formula references.
    #[must_use]
    pub fn formulae(&self) -> &[String] {
        &self.formulae
    }
    /// Exact source-qualified cask references.
    #[must_use]
    pub fn casks(&self) -> &[String] {
        &self.casks
    }
}

fn token(path: &str) -> Result<&str, ContractError> {
    if path
        .split('/')
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(failure("homebrew_source_target_invalid"));
    }
    let name = path
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".rb"))
        .ok_or_else(|| failure("homebrew_source_target_invalid"))?;
    if name.is_empty()
        || !name.starts_with(|ch: char| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        || !name.bytes().all(|ch| {
            ch.is_ascii_lowercase()
                || ch.is_ascii_digit()
                || matches!(ch, b'-' | b'_' | b'.' | b'+' | b'@')
        })
    {
        return Err(failure("homebrew_source_target_invalid"));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_targets_are_scoped_and_reject_zero_unsafe_and_duplicates() {
        let tap = TapIdentity::from_repository("owner/homebrew-tap").expect("safe tap");
        assert!(is_target_source("Formula/tool.rb"));
        assert!(!is_target_source("Formula/tool.RB"));
        assert!(!is_target_source("Casks/tool.Rb"));
        assert_eq!(
            TapIdentity::from_repository("OWNER/Homebrew-Tap").expect("canonical tap"),
            tap
        );
        let paths = ["Formula/tool@1.2.rb", "Casks/tool.rb"].map(str::to_owned);
        let targets = SourceTargets::from_inventory(&paths, &tap).expect("scoped targets");
        assert_eq!(targets.formulae(), ["owner/tap/tool@1.2"]);
        assert_eq!(targets.casks(), ["owner/tap/tool"]);
        for paths in [
            vec!["Brewfile"],
            vec!["Formula/tool.RB"],
            vec!["Formula/-option.rb"],
            vec!["Formula/UPPER.rb"],
            vec!["Formula/$(cmd).rb"],
            vec!["Formula/a.rb", "Formula/nested/a.rb"],
            vec!["Formula/../a.rb"],
        ] {
            assert!(
                SourceTargets::from_inventory(
                    &paths.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                    &tap
                )
                .is_err()
            );
        }
        let phases = super::super::audit::audit_arguments(&targets);
        assert_eq!(phases.len(), 3);
        assert_eq!(phases[0].1, ["audit", "--strict", "--online"]);
        assert!(phases[1].1.contains(&"--formula".to_owned()));
        assert!(phases[2].1.contains(&"--cask".to_owned()));
        assert!(
            phases[1..]
                .iter()
                .all(|(_, argv)| argv.contains(&"--skip-style".to_owned()))
        );
    }
}
