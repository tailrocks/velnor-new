use super::FetchRequest;
use crate::OrchestratorError;
use crate::internal::internal;

pub(super) fn owned_fetch_env(
    request: &FetchRequest,
    get: &dyn Fn(&str) -> Option<String>,
) -> Result<Vec<(String, String)>, OrchestratorError> {
    let mut owned = Vec::new();
    for key in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
        "MISE_AUTO_INSTALL",
        "MISE_EXEC_AUTO_INSTALL",
        "MISE_RUSTUP_HOME",
        "MISE_CARGO_HOME",
        "RUSTUP_TOOLCHAIN",
    ] {
        let value = if key == "MISE_CARGO_HOME" {
            Some(
                request
                    .managed_cargo_home
                    .to_str()
                    .ok_or_else(|| internal("private_fetch_cargo_home_invalid"))?
                    .to_owned(),
            )
        } else {
            get(key)
        };
        if let Some(value) = value {
            owned.push((key.to_owned(), value));
        }
    }
    Ok(owned)
}
