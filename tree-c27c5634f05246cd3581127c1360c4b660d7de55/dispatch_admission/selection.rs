use super::ledger::{AdmissionEntry, AdmissionKind, TerminalOutcome};
use super::observation::{ExecutableObservation, executable};
use eyre::{Result, bail};
use mbx_cache_core::{AdapterKind, UnitIdentity};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub(crate) struct SelectionAttempt {
    entry: AdmissionEntry,
}

#[derive(Serialize)]
struct Selection {
    schema_version: u8,
    kind: &'static str,
    selected_shim: ExecutableObservation,
    real_driver: ExecutableObservation,
    named_environment: Vec<(String, PathBuf)>,
    unit: Option<UnitIdentity>,
    supplied_sdk_roots: Vec<PathBuf>,
}

impl SelectionAttempt {
    pub(crate) fn begin() -> Result<Option<Self>> {
        Ok(
            AdmissionEntry::from_environment(AdapterKind::Cc, AdmissionKind::CcSelection)?
                .map(|entry| Self { entry }),
        )
    }

    /// Observe the actual consumed route; this does not infer every possible CC route.
    pub(crate) fn selected(
        self,
        shim: &Path,
        real_driver: &Path,
        named_environment: Vec<(String, PathBuf)>,
        unit: Option<UnitIdentity>,
        sdk_roots: Vec<PathBuf>,
    ) -> Result<()> {
        if named_environment.len() > 256 || sdk_roots.len() > 256 {
            bail!("CC selection evidence exceeds bounded configuration");
        }
        let selected_shim = executable(shim)?;
        let owner = executable(&std::env::current_exe()?)?;
        if selected_shim.sha256 != owner.sha256 {
            bail!("actual selected CC shim differs from owning executable");
        }
        // The selected compiler may use a normal toolchain alias. Record bytes
        // from its resolved executable without treating it as a managed shim.
        let real_driver = executable(&real_driver.canonicalize()?)?;
        if real_driver.sha256 == owner.sha256 {
            bail!("actual CC driver resolves recursively to owning shim");
        }
        self.entry.evidence(&Selection {
            schema_version: 1,
            kind: "actual_cc_selection",
            selected_shim,
            real_driver,
            named_environment,
            unit,
            supplied_sdk_roots: sdk_roots,
        })
    }

    pub(crate) fn unavailable(self, reason: &str) -> Result<()> {
        self.entry.finish(TerminalOutcome::Failed {
            reason: reason.into(),
        })
    }
}
