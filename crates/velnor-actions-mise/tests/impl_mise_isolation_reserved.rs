//! Privileged declarations fail before spawning a project task.

use super::*;

#[test]
fn hook_escape_privileged_declared_keys_never_run() {
    for key in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
        "RUSTUP_AUTO_INSTALL",
        "MISE_AUTO_INSTALL",
        "MISE_EXEC_AUTO_INSTALL",
        "MISE_GITHUB_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "CARGO_REGISTRY_TOKEN",
    ] {
        assert!(is_reserved_env_key(key), "{key} must be reserved");
        let declared = vec![(OsString::from(key), OsString::from("hostile"))];
        assert!(
            matches!(
                IsolatedCommand::repo_task("sh", Vec::new(), &declared),
                Err(MiseError::InvalidStepInput { .. })
            ),
            "project task declaring {key} must fail before spawn"
        );
    }
}

#[test]
fn rustup_auto_install_declarations_cannot_override_the_fixed_overlay() {
    for value in ["0", "false", ""] {
        let declared = [(OsString::from("RUSTUP_AUTO_INSTALL"), OsString::from(value))];
        assert!(
            matches!(
                IsolatedCommand::repo_task("sh", Vec::new(), &declared),
                Err(MiseError::InvalidStepInput { field, value })
                    if field == "RUSTUP_AUTO_INSTALL" && value == "reserved_env_key"
            ),
            "caller declaration {value:?} must fail before spawn"
        );
    }
}
