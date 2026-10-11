//! Whole-template validation for action-owned MBX lifecycle commands.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, commands};

/// Resolve the executable installed by the pinned native action.
pub(crate) const RESOLVE_ACTION_MBX_BIN: &str = "if ! mbx_bin=$(command -v mbx); then printf '%s\\n' 'action-owned MBX is not on PATH' >&2; exit 1; fi; case \"$mbx_bin\" in /*) ;; *) printf '%s\\n' 'action-owned MBX path must be absolute' >&2; exit 1 ;; esac; if [ ! -f \"$mbx_bin\" ] || [ ! -x \"$mbx_bin\" ]; then printf '%s\\n' 'action-owned MBX path must be an executable file' >&2; exit 1; fi";

/// Shell-local absolute path assigned by [`RESOLVE_ACTION_MBX_BIN`].
pub(crate) const ACTION_MBX_BIN: &str = "$mbx_bin";

const RUST_EXEC_PREFIX: &str = "mise --no-config --no-env --no-hooks exec rust@";
const VERSION_LINE_PREFIX: &str = "[ \"$version_line\" = 'mbx ";
const VERSION_LINE_SUFFIX: &str = "' ] ||";
const PRESEED_VERSION_PREFIX: &str = "grep -qxF \"mbx ";
const PRESEED_VERSION_SUFFIX: &str = "\"";

/// Validate a fixed internally-built shell vector before its role is attached.
/// The serialized form is checked again against a role-specific producer.
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

/// Validate a serialized MBX role by rebuilding its complete canonical argv.
pub(crate) fn validate_argv_for_role(
    argv: &[String],
    role: Option<StepRole>,
) -> Result<(), RenderError> {
    validate_argv(argv)?;
    let script_at =
        commands::inline_shell_script_at(argv).ok_or_else(|| bad_shape("not_inline_shell"))?;
    let script = &argv[script_at];
    let rust = rust_toolchain(script)?;
    let expected = match role {
        Some(StepRole::MbxVersionCheck) => {
            let version = value_between(script, VERSION_LINE_PREFIX, VERSION_LINE_SUFFIX)?;
            shell_run(crate::cache_steps::mbx_version_check_step(
                version,
                rust,
                BTreeMap::new(),
            )?)?
        }
        Some(StepRole::MbxWorkspaceCleanup) => {
            let action = Step {
                name: crate::cache_steps::MBX_RESTORE_NAME.to_owned(),
                id: None,
                role: Some(StepRole::MbxCache),
                condition: None,
                kind: StepKind::Action {
                    uses: format!("{}@{}", crate::cache_steps::MBX_ACTION_NAME, "a".repeat(40)),
                    with: BTreeMap::from([
                        ("version".to_owned(), "1.0.0".to_owned()),
                        ("toolchain".to_owned(), rust.to_owned()),
                    ]),
                    env: BTreeMap::new(),
                },
            };
            shell_run(crate::cache_steps::mbx_workspace_clean_step_for_action(
                &action,
            )?)?
        }
        Some(StepRole::PreseedBuild) => {
            let build = candidate_build_argv(rust);
            shell_run(crate::preseed::preseed_build_step(
                &build,
                &BTreeMap::new(),
            )?)?
        }
        Some(StepRole::PreseedVerifyBuild) => {
            let version = value_between(script, PRESEED_VERSION_PREFIX, PRESEED_VERSION_SUFFIX)?;
            let probe = mbx_probe_argv(rust);
            shell_run(crate::preseed::preseed_verify_step(
                &probe,
                version,
                &BTreeMap::new(),
            )?)?
        }
        _ => return Err(bad_shape("role_not_authorized")),
    };
    if argv != expected {
        return Err(bad_shape("canonical_template_mismatch"));
    }
    Ok(())
}

/// Validate a shell step using its role when the role owns an MBX template.
///
/// All other steps retain the generic fixed-argv validator. A role never
/// grants a partial shell exception: MBX roles must match a complete
/// producer-generated template in [`validate_argv_for_role`].
pub(crate) fn validate_step_argv(
    argv: &[String],
    role: Option<StepRole>,
) -> Result<(), RenderError> {
    if role_uses_resolver(role) {
        validate_argv_for_role(argv, role)
    } else {
        commands::validate_command_argv(argv)
    }
}

/// Whether this role selects one of the four complete MBX templates.
fn role_uses_resolver(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::MbxVersionCheck
                | StepRole::MbxWorkspaceCleanup
                | StepRole::PreseedBuild
                | StepRole::PreseedVerifyBuild
        )
    )
}

fn rust_toolchain(script: &str) -> Result<&str, RenderError> {
    let (_, after) = script
        .split_once(RUST_EXEC_PREFIX)
        .ok_or_else(|| bad_shape("rust_toolchain_missing"))?;
    let (rust, _) = after
        .split_once(" -- ")
        .ok_or_else(|| bad_shape("rust_toolchain_unclosed"))?;
    if !is_exact_version(rust) {
        return Err(bad_shape("rust_toolchain_invalid"));
    }
    Ok(rust)
}

fn value_between<'a>(script: &'a str, prefix: &str, suffix: &str) -> Result<&'a str, RenderError> {
    if script.matches(prefix).count() != 1 {
        return Err(bad_shape("dynamic_value_not_unique"));
    }
    let (_, after) = script
        .split_once(prefix)
        .ok_or_else(|| bad_shape("dynamic_value_missing"))?;
    let (value, _) = after
        .split_once(suffix)
        .ok_or_else(|| bad_shape("dynamic_value_unclosed"))?;
    if !is_exact_version(value) {
        return Err(bad_shape("dynamic_value_invalid"));
    }
    Ok(value)
}

fn is_exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn candidate_build_argv(rust: &str) -> Vec<String> {
    let rust_spec = format!("rust@{rust}");
    let rust_selector = format!("+{rust}");
    let argv = [
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        rust_spec,
        "--".to_owned(),
        "mbx".to_owned(),
        rust_selector,
        "build".to_owned(),
        "--release".to_owned(),
        "--locked".to_owned(),
        "--package".to_owned(),
        "velnor-actions-cli".to_owned(),
        "--bin".to_owned(),
        "velnor-actions".to_owned(),
    ];
    argv.to_vec()
}

fn mbx_probe_argv(rust: &str) -> Vec<String> {
    let argv = [
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        format!("rust@{rust}"),
        "--".to_owned(),
        "mbx".to_owned(),
        "--version".to_owned(),
    ];
    argv.to_vec()
}

fn shell_run(step: Step) -> Result<Vec<String>, RenderError> {
    match step.kind {
        StepKind::Shell { run, .. } => Ok(run),
        _ => Err(bad_shape("canonical_step_not_shell")),
    }
}

fn bad_shape(reason: &str) -> RenderError {
    RenderError::BadCommand(format!("action_owned_mbx_binding:{reason}"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{Step, StepKind, StepRole};

    use super::{
        ACTION_MBX_BIN, RESOLVE_ACTION_MBX_BIN, candidate_build_argv, mbx_probe_argv,
        role_uses_resolver, validate_argv, validate_argv_for_role, validate_step_argv,
    };
    use crate::{RenderError, cache_steps};

    fn version_check() -> Step {
        cache_steps::mbx_version_check_step("1.23.0", "1.99.0", BTreeMap::new())
            .expect("version check")
    }

    fn cleanup() -> Step {
        let action = Step {
            name: cache_steps::MBX_RESTORE_NAME.to_owned(),
            id: None,
            role: Some(StepRole::MbxCache),
            condition: None,
            kind: StepKind::Action {
                uses: format!("{}@{}", cache_steps::MBX_ACTION_NAME, "a".repeat(40)),
                with: BTreeMap::from([
                    ("version".to_owned(), "1.23.0".to_owned()),
                    ("toolchain".to_owned(), "1.99.0".to_owned()),
                ]),
                env: BTreeMap::new(),
            },
        };
        cache_steps::mbx_workspace_clean_step_for_action(&action).expect("cleanup")
    }

    fn build() -> Step {
        crate::preseed::preseed_build_step(&candidate_build_argv("1.99.0"), &BTreeMap::new())
            .expect("preseed build")
    }

    fn verify_build() -> Step {
        crate::preseed::preseed_verify_step(&mbx_probe_argv("1.99.0"), "1.23.0", &BTreeMap::new())
            .expect("preseed verification")
    }

    fn run(step: &Step) -> &[String] {
        let StepKind::Shell { run, .. } = &step.kind else {
            panic!("MBX template must be shell");
        };
        run
    }

    #[test]
    fn whole_templates_accept_all_four_producer_roles() {
        for step in [version_check(), cleanup(), build(), verify_build()] {
            assert!(role_uses_resolver(step.role));
            validate_argv_for_role(run(&step), step.role).expect("canonical MBX template");
            crate::commands::join_validated_argv_for_run(run(&step))
                .expect("canonical template joins");
        }
    }

    #[test]
    fn plain_step_renderer_uses_role_validation_and_keeps_generic_rejection() {
        let step = version_check();
        crate::steps_plain::plain_step_to_yaml(&step)
            .expect("plain renderer accepts the complete action-owned template");

        let mut forged = step;
        let StepKind::Shell { run, .. } = &mut forged.kind else {
            panic!("MBX template must be shell");
        };
        run[2].push_str("; echo injected");
        assert!(crate::steps_plain::plain_step_to_yaml(&forged).is_err());

        let generic = Step {
            name: "generic shell".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["sh".to_owned(), "-c".to_owned(), "echo $(evil)".to_owned()],
                env: BTreeMap::new(),
            },
        };
        assert!(crate::steps_plain::plain_step_to_yaml(&generic).is_err());
    }

    #[test]
    fn forged_role_rejects_suffix_order_reassignment_and_comment_payloads() {
        let step = version_check();
        let mut suffix = run(&step).to_vec();
        suffix[2].push_str("; echo injected");
        assert!(validate_argv_for_role(&suffix, step.role).is_err());

        let mut before_resolver = run(&step).to_vec();
        let resolver_at = before_resolver[2]
            .find(RESOLVE_ACTION_MBX_BIN)
            .expect("resolver in canonical script");
        before_resolver[2].insert_str(resolver_at, "\"$mbx_bin\" --version; ");
        assert!(validate_argv_for_role(&before_resolver, step.role).is_err());

        let mut reassigned = run(&step).to_vec();
        let resolver_end = reassigned[2]
            .find(RESOLVE_ACTION_MBX_BIN)
            .expect("resolver in canonical script")
            + RESOLVE_ACTION_MBX_BIN.len();
        reassigned[2].insert_str(resolver_end, "; mbx_bin=/tmp/evil");
        assert!(validate_argv_for_role(&reassigned, step.role).is_err());

        let mut comment_only = run(&step).to_vec();
        assert!(comment_only[2].contains(" -- \"$mbx_bin\" --version"));
        comment_only[2] = comment_only[2].replace(
            " -- \"$mbx_bin\" --version",
            "; : ' -- \"$mbx_bin\"'; echo injected",
        );
        assert!(comment_only[2].contains("; : ' -- \"$mbx_bin\"'"));
        assert!(validate_argv_for_role(&comment_only, step.role).is_err());
    }

    #[test]
    fn canonical_template_without_an_authorized_role_is_rejected() {
        let step = version_check();
        assert!(validate_argv_for_role(run(&step), None).is_err());
    }

    #[test]
    fn generic_steps_still_reject_command_substitution() {
        let argv = vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("echo $(evil); {ACTION_MBX_BIN} --version"),
        ];
        assert!(matches!(
            validate_step_argv(&argv, None),
            Err(RenderError::BadCommand(message)) if message.starts_with("command_substitution:")
        ));
    }

    #[test]
    fn existing_resolver_fragment_remains_generic_validation_only() {
        let step = version_check();
        assert!(matches!(
            crate::commands::validate_command_argv(run(&step)),
            Err(RenderError::BadCommand(message)) if message.starts_with("command_substitution:")
        ));
        validate_argv(run(&step)).expect("typed construction accepts its resolver");
    }
}
