//! Missing source SDK qualification is an unavailable execution authority.

use crate::OrchestratorError;

/// No variants and no constructor: no current issuer can grant this authority.
pub(super) enum QualifiedExecutionInventory {}

/// Only compiled source qualification may introduce a future inventory issuer.
pub(super) fn acquire() -> Result<QualifiedExecutionInventory, OrchestratorError> {
    Err(OrchestratorError::NeedsCargo {
        problem: "current_rust_inventory_source_sdk_qualification_missing".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_source_sdk_inventory_denies_without_cargo() {
        let probe = crate::inventory::cargo_probe::CargoProbe::begin();
        let error = acquire().err().expect("missing qualification");
        assert!(matches!(
            error,
            OrchestratorError::NeedsCargo { problem }
                if problem == "current_rust_inventory_source_sdk_qualification_missing"
        ));
        assert_eq!(probe.attempts(), 0);
    }
}
