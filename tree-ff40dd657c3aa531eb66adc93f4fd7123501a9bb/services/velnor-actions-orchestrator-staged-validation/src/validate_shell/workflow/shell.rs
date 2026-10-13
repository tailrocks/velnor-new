use velnor_actions_orchestrator_core::OrchestratorError;

use super::super::{shellcheck_fail, unquote_run_scalar};
use super::ShellDialect;

pub(super) fn parse_shell(value: &str) -> Result<ShellDialect, OrchestratorError> {
    let shell = unquote_run_scalar(value)?;
    match shell.as_str() {
        "bash" | "bash -e {0}" => Ok(ShellDialect::Bash),
        "sh" | "sh -e {0}" => Ok(ShellDialect::Sh),
        _ => Err(shellcheck_fail("shell_form_unsupported")),
    }
}

pub(super) fn reject_shellcheck_shell_directive(body: &str) -> Result<(), OrchestratorError> {
    if body.lines().any(shellcheck_overrides_shell) {
        Err(shellcheck_fail("shellcheck_shell_directive_unsupported"))
    } else {
        Ok(())
    }
}

fn shellcheck_overrides_shell(line: &str) -> bool {
    let Some(comment) = line.trim_start().strip_prefix('#') else {
        return false;
    };
    let lower = comment.trim_start().to_ascii_lowercase();
    let Some(payload) = lower.strip_prefix("shellcheck") else {
        return false;
    };
    if payload
        .chars()
        .next()
        .is_some_and(|character| !character.is_ascii_whitespace())
    {
        return false;
    }
    let fields: Vec<_> = payload
        .split(|character: char| character.is_ascii_whitespace() || character == ',')
        .filter(|field| !field.is_empty())
        .collect();
    fields.iter().enumerate().any(|(index, field)| {
        field.starts_with("shell=")
            || (*field == "shell"
                && fields
                    .get(index + 1)
                    .is_some_and(|next| next.starts_with('=')))
    })
}
