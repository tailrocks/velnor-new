//! Extension-input resolution states and verified construction (P03).
//!
//! Declared via `#[path]` from `identity.rs` (no `lib.rs` edit).
//! Reports which semantic inputs an extension leaves unresolved
//! (`None`/empty is unknown, never proven absent) and validates
//! identity paths before construction.

use velnor_actions_contract::{ContractError, normalize_posix_path};

use super::GroupExtensionInputs;
use crate::task_identity::RustTaskIdentityExtension;
use crate::tasks::TaskGroup;

/// One semantic input an extension leaves unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedInput {
    /// Lockfile digest unknown.
    Lockfile,
    /// Nextest-config digest unknown for a Nextest task.
    NextestConfig,
    /// Archive source unknown for a Nextest `Build` task.
    ArchiveSource,
    /// Build-script `rerun-if-changed` inputs unknown.
    RerunInputs,
}

impl UnresolvedInput {
    /// Stable vocabulary word for error codes and logs.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lockfile => "lockfile",
            Self::NextestConfig => "nextest_config",
            Self::ArchiveSource => "archive_source",
            Self::RerunInputs => "rerun_inputs",
        }
    }

    /// Whether this input blocks reuse and coverage decisions.
    ///
    /// Only [`UnresolvedInput::ArchiveSource`] reports without
    /// blocking: production never populates `archive_source`
    /// (`ExtensionBundle::inputs` hardcodes `None`) and binds
    /// archives through the dedicated
    /// `check_archive_identity_with_source` gate instead, so
    /// treating it as blocking here would refuse every Nextest
    /// `Build` unconditionally.
    #[must_use]
    pub fn blocks_gates(&self) -> bool {
        !matches!(self, Self::ArchiveSource)
    }
}

impl RustTaskIdentityExtension {
    /// First gate-blocking unresolved input, in stable inventory order.
    ///
    /// The single enforcement point behind [`unresolved_inputs`]:
    /// reuse and coverage gates call this instead of reimplementing
    /// the slot checks, so the inventory and the gates cannot drift.
    /// [`UnresolvedInput::blocks_gates`] names the blocking subset.
    #[must_use]
    pub fn first_blocking_input(&self) -> Option<UnresolvedInput> {
        unresolved_inputs(self)
            .into_iter()
            .find(UnresolvedInput::blocks_gates)
    }
}

/// Semantic inputs `ext` leaves unresolved, in stable order.
///
/// Unknown means unobserved: the orchestrator resolves each against
/// the checkout (content digest or proven absence) before reuse or
/// coverage may proceed.
#[must_use]
pub fn unresolved_inputs(ext: &RustTaskIdentityExtension) -> Vec<UnresolvedInput> {
    let mut unresolved = Vec::new();
    if ext.lock_slot.is_unknown() {
        unresolved.push(UnresolvedInput::Lockfile);
    }
    if ext.task_kind == crate::tasks::TaskKind::Nextest && ext.nextest_slot.is_unknown() {
        unresolved.push(UnresolvedInput::NextestConfig);
    }
    if ext.task_kind == crate::tasks::TaskKind::Build
        && ext.test_runner == crate::profile::TestRunner::CargoNextest
        && ext.archive.is_none()
    {
        unresolved.push(UnresolvedInput::ArchiveSource);
    }
    if ext.undeclared_reads {
        unresolved.push(UnresolvedInput::RerunInputs);
    }
    unresolved
}

/// Normalize one identity path: repo-relative, explicit rejects.
///
/// Case and Unicode pass through byte-for-byte; empty, absolute,
/// traversing, backslash, and NUL/control-carrying paths fail.
///
/// # Errors
///
/// Returns [`ContractError`] for malformed checkout paths.
pub fn normalize_identity_path(path: &str) -> Result<String, ContractError> {
    if path.is_empty() {
        return Err(ContractError::identity("path", "empty_path"));
    }
    if path.contains('\0') || path.chars().any(char::is_control) {
        return Err(ContractError::identity("path", "control_characters"));
    }
    if path.contains('\\') {
        return Err(ContractError::identity("path", "backslash_separator"));
    }
    normalize_posix_path(path)
}

impl TaskGroup {
    /// Derive the identity extension, validating every identity path.
    ///
    /// The manifest, declared inputs, and rerun inputs must be
    /// well-formed checkout paths; anything else fails instead of
    /// entering the identity preimage silently.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for malformed identity paths.
    pub fn identity_extension_verified(
        &self,
        inputs: &GroupExtensionInputs<'_>,
    ) -> Result<RustTaskIdentityExtension, ContractError> {
        normalize_identity_path(inputs.manifest)?;
        for path in self.declared_inputs.iter().chain(inputs.targets.iter()) {
            normalize_identity_path(path)?;
        }
        if let Some(rerun) = inputs.rerun_inputs {
            for path in rerun {
                normalize_identity_path(path)?;
            }
        }
        Ok(self.identity_extension(inputs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_identity::DigestSlot;

    /// Extension inputs with `lock`, `nextest`, and `rerun` supplied.
    fn inputs(
        lock: crate::task_identity::DigestSlot,
        nextest: crate::task_identity::DigestSlot,
        rerun: Option<&[String]>,
        build_script: bool,
    ) -> GroupExtensionInputs<'_> {
        GroupExtensionInputs {
            package_id: "demo",
            workspace_id: "workspace",
            profile: "default",
            manifest: "Cargo.toml",
            graph_digest: "graph",
            targets: &[],
            config_digest: "config",
            lock_digest: lock,
            nextest_digest: nextest,
            archive_source: None,
            rerun_inputs: rerun,
            has_build_script: build_script,
        }
    }

    /// Minimal group of `kind` with the Nextest runner.
    fn group(kind: crate::tasks::TaskKind) -> TaskGroup {
        TaskGroup {
            task_id: "stack/rust/root/nextest/default".to_owned(),
            package_id: "demo".to_owned(),
            package_name: "demo".to_owned(),
            manifest_key: "root".to_owned(),
            kind,
            configuration: "default".to_owned(),
            features: Vec::new(),
            target: "host".to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: None,
            compile_driver: crate::profile::CompileDriver::Cargo,
            test_runner: crate::profile::TestRunner::CargoNextest,
            declared_inputs: Vec::new(),
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
            nextest_profile: crate::profile::NextestProfile::Default,
            run_ignored: None,
        }
    }

    #[test]
    fn unresolved_inventory_names_unknown_inputs() {
        let ext = group(crate::tasks::TaskKind::Nextest).identity_extension(&inputs(
            DigestSlot::Unknown("unprobed".to_owned()),
            DigestSlot::Unknown("unprobed".to_owned()),
            Some(&[]),
            false,
        ));
        assert_eq!(
            unresolved_inputs(&ext),
            vec![UnresolvedInput::Lockfile, UnresolvedInput::NextestConfig]
        );
        let ext = group(crate::tasks::TaskKind::Test).identity_extension(&inputs(
            DigestSlot::Known("lock".to_owned()),
            DigestSlot::Unknown("unprobed".to_owned()),
            Some(&[]),
            false,
        ));
        assert_eq!(
            unresolved_inputs(&ext),
            [] as [crate::identity::identity_closure::UnresolvedInput; 0]
        );
        let build = group(crate::tasks::TaskKind::Build).identity_extension(&inputs(
            DigestSlot::Known("lock".to_owned()),
            DigestSlot::Known("nextest".to_owned()),
            Some(&[]),
            false,
        ));
        assert_eq!(
            unresolved_inputs(&build),
            vec![UnresolvedInput::ArchiveSource]
        );
        let script = group(crate::tasks::TaskKind::Clippy).identity_extension(&inputs(
            DigestSlot::Known("lock".to_owned()),
            DigestSlot::Unknown("unprobed".to_owned()),
            None,
            true,
        ));
        assert!(unresolved_inputs(&script).contains(&UnresolvedInput::RerunInputs));
    }

    #[test]
    fn proven_absence_binds_without_blocking() {
        let ext = group(crate::tasks::TaskKind::Nextest).identity_extension(&inputs(
            DigestSlot::AbsentProven("not_found:Cargo.lock".to_owned()),
            DigestSlot::AbsentProven("not_found:.config/nextest.toml".to_owned()),
            Some(&[]),
            false,
        ));
        assert_eq!(
            unresolved_inputs(&ext),
            [] as [crate::identity::identity_closure::UnresolvedInput; 0]
        );
        assert_eq!(ext.lock_digest, None);
        assert_eq!(ext.nextest_digest, None);
        let unknown = group(crate::tasks::TaskKind::Nextest).identity_extension(&inputs(
            DigestSlot::Unknown("unprobed".to_owned()),
            DigestSlot::AbsentProven("not_found:.config/nextest.toml".to_owned()),
            Some(&[]),
            false,
        ));
        assert_eq!(unresolved_inputs(&unknown), vec![UnresolvedInput::Lockfile]);
    }

    #[test]
    fn unresolved_ignores_composite_spellings() {
        let mut cargo = group(crate::tasks::TaskKind::Build);
        cargo.test_runner = crate::profile::TestRunner::CargoTest;
        let mut spoofed = cargo.identity_extension(&inputs(
            DigestSlot::Known("lock".to_owned()),
            DigestSlot::Known("nextest".to_owned()),
            Some(&[]),
            false,
        ));
        spoofed.driver = "cargo+nextest-spoof".to_owned();
        spoofed.kind = "build".to_owned();
        assert!(
            !unresolved_inputs(&spoofed).contains(&UnresolvedInput::ArchiveSource),
            "substring sniffing must not resurrect: {spoofed:?}"
        );
        let mut test = group(crate::tasks::TaskKind::Test);
        test.test_runner = crate::profile::TestRunner::CargoTest;
        let mut kind_spoof = test.identity_extension(&inputs(
            DigestSlot::Unknown("unprobed".to_owned()),
            DigestSlot::Unknown("unprobed".to_owned()),
            Some(&[]),
            false,
        ));
        kind_spoof.kind = "nextest".to_owned();
        assert_eq!(
            unresolved_inputs(&kind_spoof),
            vec![UnresolvedInput::Lockfile]
        );
    }

    #[test]
    fn verified_construction_rejects_bad_paths() {
        let clippy = group(crate::tasks::TaskKind::Clippy);
        assert!(
            clippy
                .identity_extension_verified(&inputs(
                    DigestSlot::Known("l".to_owned()),
                    DigestSlot::Unknown("unprobed".to_owned()),
                    Some(&[]),
                    false
                ))
                .is_ok()
        );
        let group = TaskGroup {
            declared_inputs: vec!["../escape".to_owned()],
            ..clippy.clone()
        };
        assert!(
            group
                .identity_extension_verified(&inputs(
                    DigestSlot::Known("l".to_owned()),
                    DigestSlot::Unknown("unprobed".to_owned()),
                    Some(&[]),
                    false
                ))
                .is_err()
        );
        assert!(normalize_identity_path("Crates/Äpfel/x.proto").is_ok());
        assert!(normalize_identity_path("a\\b").is_err());
    }

    #[test]
    fn identity_paths_preserve_case_and_reject_malformed() {
        assert_eq!(
            normalize_identity_path("Crates/Äpfel/Cargo.toml").expect("unicode"),
            "Crates/Äpfel/Cargo.toml"
        );
        for bad in ["", "/abs/path", "a/../b", "a\\b", "a\0b", "a\nb"] {
            assert!(normalize_identity_path(bad).is_err(), "{bad:?}");
        }
    }
}
