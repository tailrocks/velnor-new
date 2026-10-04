//! Canonical Rustup policy in every qualified helper execution envelope.
use crate::RenderError;
use velnor_actions_contract::CompiledSourceHelper;

pub(super) fn validate(record: &CompiledSourceHelper) -> Result<(), RenderError> {
    let key = "RUSTUP_AUTO_INSTALL";
    let mut environments = std::iter::once(record.environment()).chain(
        record
            .execution_recipe()
            .map(velnor_actions_contract::CompiledNativeExecRecipe::environment),
    );
    if environments.any(|environment| environment.get(key).is_some_and(|value| value != "0")) {
        return Err(invalid());
    }
    for argument in record.invocation().execution_prefix() {
        if let Some(value) = argument.strip_prefix("RUSTUP_AUTO_INSTALL=") {
            let bound = matches!(value, "$RUSTUP_AUTO_INSTALL" | "${RUSTUP_AUTO_INSTALL}")
                && record
                    .environment()
                    .get(key)
                    .is_some_and(|value| value == "0");
            if value != "0" && !bound {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn invalid() -> RenderError {
    RenderError::BadCommand("source_helper_rustup_auto_install".to_owned())
}

#[cfg(test)]
#[path = "source_helper_rustup_tests.rs"]
mod tests;
