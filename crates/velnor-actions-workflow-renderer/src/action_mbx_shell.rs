//! Narrow validation for the action-owned MBX executable binding.

use crate::{RenderError, commands};

/// Resolve the executable installed by the pinned native action.
pub(crate) const RESOLVE_ACTION_MBX_BIN: &str = "if ! mbx_bin=$(command -v mbx); then printf '%s\\n' 'action-owned MBX is not on PATH' >&2; exit 1; fi; case \"$mbx_bin\" in /*) ;; *) printf '%s\\n' 'action-owned MBX path must be absolute' >&2; exit 1 ;; esac; if [ ! -f \"$mbx_bin\" ] || [ ! -x \"$mbx_bin\" ]; then printf '%s\\n' 'action-owned MBX path must be an executable file' >&2; exit 1; fi";

/// Shell-local absolute path assigned by [`RESOLVE_ACTION_MBX_BIN`].
pub(crate) const ACTION_MBX_BIN: &str = "$mbx_bin";

/// Validate the exact internal resolver while leaving generic validation strict.
pub(crate) fn validate_argv(argv: &[String]) -> Result<(), RenderError> {
    let script_at =
        commands::inline_shell_script_at(argv).ok_or_else(|| bad_shape("not_inline_shell"))?;
    let script = &argv[script_at];
    if script.matches(RESOLVE_ACTION_MBX_BIN).count() != 1 || !script.contains(" -- \"$mbx_bin\"") {
        return Err(bad_shape("resolver_or_binary_binding"));
    }

    let mut validated = argv.to_vec();
    validated[script_at] = script.replace(RESOLVE_ACTION_MBX_BIN, "ACTION_MBX_RESOLVER");
    commands::validate_command_argv(&validated)
}

/// Validate and join a typed MBX resolver command for a workflow `run:` value.
pub(crate) fn join_argv_for_run(argv: &[String]) -> Result<String, RenderError> {
    validate_argv(argv)?;
    commands::join_validated_argv_for_run(argv)
}

/// Whether this step role is authorized to carry the fixed MBX resolver.
pub(crate) fn role_uses_resolver(role: Option<velnor_actions_contract::StepRole>) -> bool {
    matches!(
        role,
        Some(
            velnor_actions_contract::StepRole::MbxVersionCheck
                | velnor_actions_contract::StepRole::MbxWorkspaceCleanup
                | velnor_actions_contract::StepRole::PreseedBuild
                | velnor_actions_contract::StepRole::PreseedVerifyBuild
        )
    )
}

fn bad_shape(reason: &str) -> RenderError {
    RenderError::BadCommand(format!("action_owned_mbx_binding:{reason}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{RESOLVE_ACTION_MBX_BIN, join_argv_for_run, validate_argv};
    use crate::{RenderError, commands};

    fn resolver_argv(extra: &str) -> Vec<String> {
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "set -eu; {RESOLVE_ACTION_MBX_BIN}; mise --no-config --no-env --no-hooks exec rust@1.99.0 -- \"$mbx_bin\" --version{extra}"
            ),
        ]
    }

    #[test]
    fn typed_route_accepts_only_the_fixed_action_resolver() {
        let argv = resolver_argv("");
        assert!(validate_argv(&argv).is_ok());
        let joined = join_argv_for_run(&argv).expect("typed MBX command joins");
        assert!(joined.contains(RESOLVE_ACTION_MBX_BIN));
        assert!(joined.contains("-- \"$mbx_bin\" --version"));
        assert!(matches!(
            commands::validate_command_argv(&argv),
            Err(RenderError::BadCommand(message)) if message.starts_with("command_substitution:")
        ));
        assert!(
            crate::steps_shell::action_owned_mbx_shell_step(
                "Verify native MBX",
                argv.clone(),
                BTreeMap::new(),
            )
            .is_ok()
        );
        assert!(matches!(
            crate::steps_shell::shell_step("Generic command", argv, BTreeMap::new()),
            Err(RenderError::BadCommand(message)) if message.starts_with("command_substitution:")
        ));
    }

    #[test]
    fn typed_route_rejects_extra_substitution_and_unbound_resolver() {
        assert!(matches!(
            validate_argv(&resolver_argv("; echo $(evil)")),
            Err(RenderError::BadCommand(message)) if message.starts_with("command_substitution:")
        ));

        let mut altered = resolver_argv("");
        altered[2] = altered[2].replace("command -v mbx", "command -v git");
        assert!(matches!(
            validate_argv(&altered),
            Err(RenderError::BadCommand(message)) if message == "action_owned_mbx_binding:resolver_or_binary_binding"
        ));
    }
}
