use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole};

use crate::{RenderError, commands};

pub(crate) const RESOLVE_ACTION_MBX_BIN: &str = "if ! mbx_bin=$(command -v mbx); then printf '%s\\n' 'action-owned MBX is not on PATH' >&2; exit 1; fi; case \"$mbx_bin\" in /*) ;; *) printf '%s\\n' 'action-owned MBX path must be absolute' >&2; exit 1 ;; esac; if [ ! -f \"$mbx_bin\" ] || [ ! -x \"$mbx_bin\" ]; then printf '%s\\n' 'action-owned MBX path must be an executable file' >&2; exit 1; fi";

pub(crate) const ACTION_MBX_BIN: &str = "$mbx_bin";

const RUST_EXEC_PREFIX: &str = "mise --no-config --no-env --no-hooks exec rust@";
const RUST_EXEC_QUOTED_PREFIX: &str = "mise --no-config --no-env --no-hooks exec 'rust@'\"";
const VERSION_LINE_PREFIX: &str = "[ \"$version_line\" = 'mbx ";
const VERSION_LINE_SUFFIX: &str = "' ] ||";
const PRESEED_VERSION_PREFIX: &str = "grep -qxF \"mbx ";
const PRESEED_VERSION_SUFFIX: &str = "\"";

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
            let mut with = BTreeMap::from([
                ("version".to_owned(), "1.0.0".to_owned()),
                ("toolchain".to_owned(), rust.to_owned()),
            ]);
            if let Some((_, after)) = script.split_once("; target_dir=\"")
                && let Some((td, _)) = after.split_once('"')
                && let Some(dir) = td.strip_suffix("/target")
            {
                with.insert("working-directory".to_owned(), dir.to_owned());
            }
            let action = Step {
                parallel: false,
                name: crate::cache_steps::MBX_RESTORE_NAME.to_owned(),
                id: None,
                role: Some(StepRole::MbxCache),
                condition: None,
                kind: StepKind::Action {
                    uses: format!("{}@{}", crate::cache_steps::MBX_ACTION_NAME, "a".repeat(40)),
                    with,
                    env: BTreeMap::new(),
                },
            };
            shell_run(crate::cache_steps::mbx_workspace_clean_step_for_action(
                &action,
            )?)?
        }
        Some(StepRole::MbxBundleImport) => {
            let working_dir = bundle_working_dir(script)?;
            shell_run(crate::cache_steps::mbx_bundle_import_step(
                rust,
                working_dir.as_deref(),
                BTreeMap::new(),
            )?)?
        }
        Some(StepRole::MbxBundleExport) => {
            let working_dir = bundle_working_dir(script)?;
            shell_run(crate::cache_steps::mbx_bundle_export_step(
                rust,
                working_dir.as_deref(),
                BTreeMap::new(),
            )?)?
        }
        Some(StepRole::PreseedBuild) => {
            let with_toolchain = script.contains(&format!("+{rust}"));
            let build = candidate_build_argv(rust, with_toolchain);
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

fn bundle_working_dir(script: &str) -> Result<Option<String>, RenderError> {
    const PREFIX: &str = "cd \"$GITHUB_WORKSPACE/";
    let Some((_, after)) = script.split_once(PREFIX) else {
        return Ok(None);
    };
    let (dir, _) = after
        .split_once('"')
        .ok_or_else(|| bad_shape("bundle_working_dir_unclosed"))?;
    Ok(Some(dir.to_owned()))
}

fn role_uses_resolver(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::MbxVersionCheck
                | StepRole::MbxWorkspaceCleanup
                | StepRole::MbxBundleImport
                | StepRole::MbxBundleExport
                | StepRole::PreseedBuild
                | StepRole::PreseedVerifyBuild
        )
    )
}

fn rust_toolchain(script: &str) -> Result<&str, RenderError> {
    if let Some((_, after)) = script.split_once(RUST_EXEC_QUOTED_PREFIX)
        && let Some((rust, _)) = after.split_once("\" -- ")
        && velnor_actions_contract::config::project_tool_refs::is_project_tool_ref(rust)
    {
        return Ok(rust);
    }
    let (_, after) = script
        .split_once(RUST_EXEC_PREFIX)
        .ok_or_else(|| bad_shape("rust_toolchain_missing"))?;
    let (rust, _) = after
        .split_once(" -- ")
        .ok_or_else(|| bad_shape("rust_toolchain_unclosed"))?;
    if !crate::commands::is_exact_or_project_version(rust) {
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
    if !crate::commands::is_exact_or_project_version(value) {
        return Err(bad_shape("dynamic_value_invalid"));
    }
    Ok(value)
}

fn candidate_build_argv(rust: &str, with_toolchain: bool) -> Vec<String> {
    let mut argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        format!("rust@{rust}"),
        "--".to_owned(),
        "mbx".to_owned(),
    ];
    if with_toolchain {
        argv.push(format!("+{rust}"));
    }
    argv.extend([
        "build".to_owned(),
        "--release".to_owned(),
        "--locked".to_owned(),
        "--package".to_owned(),
        "velnor-actions-cli".to_owned(),
        "--bin".to_owned(),
        "velnor-actions".to_owned(),
    ]);
    argv
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

pub(crate) fn target_dir_for_action(
    with: &BTreeMap<String, String>,
) -> Result<String, RenderError> {
    let Some(dir) = with
        .get("working-directory")
        .map(String::as_str)
        .filter(|d| !d.is_empty() && *d != ".")
    else {
        return Ok("target".to_owned());
    };
    if !dir.starts_with('/')
        && !dir.ends_with('/')
        && dir.split('/').all(|s| {
            !s.is_empty()
                && s != "."
                && s != ".."
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
    {
        Ok(format!("{dir}/target"))
    } else {
        Err(RenderError::BadCommand(format!("bad_working_dir:{dir}")))
    }
}

fn bad_shape(reason: &str) -> RenderError {
    RenderError::BadCommand(format!("action_owned_mbx_binding:{reason}"))
}
