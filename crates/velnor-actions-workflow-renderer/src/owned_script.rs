//! Serialization for renderer-owned fixed shell scripts.
//!
//! General argv validation rejects control characters. This private path
//! carries fixed multi-line scripts as octal-encoded `printf` arguments, so
//! the emitted `bash -c` vector remains single-line and passes the ordinary
//! renderer checks without widening task-command input.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use crate::RenderError;

/// Build a scrubbed Bash step for fixed scripts owned by this renderer.
///
/// Callers must supply a generated script, never repository or user text.
/// Each source line becomes one literal printf argument; Bash executes the
/// resulting bytes in a child shell. The outer argv stays single-line.
/// # Errors
pub(crate) fn owned_bash_script_step(
    name: &str,
    script: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if script.is_empty() || script.contains('\0') || script.contains('\r') {
        return Err(RenderError::BadCommand("invalid_owned_script".to_owned()));
    }
    crate::steps::scan_for_private_subcommands(script)?;
    let lines = script
        .split('\n')
        .map(encode_printf_octal)
        .map(|line| format!("'{line}'"))
        .collect::<Vec<_>>()
        .join(" ");
    let wrapper = format!("set -o pipefail; printf '%b\\n' {lines} | bash");
    crate::steps::shell_step(name, vec!["bash".to_owned(), "-c".to_owned(), wrapper], env)
}

/// Encode bytes as fixed-width octal escapes for Bash `printf %b`.
fn encode_printf_octal(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("\\{byte:03o}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, process::Command};

    use velnor_actions_contract::StepKind;

    use super::owned_bash_script_step;

    #[test]
    fn owned_script_is_single_line_and_executes_without_expanding_literals() {
        let script = "printf '%s\\n' 'first line'\nprintf '%s\\n' '$(printf injected >&2)'\n";
        let step = owned_bash_script_step("fixed script", script, BTreeMap::new())
            .expect("owned script builds");
        let StepKind::Shell { run, .. } = step.kind else {
            panic!("script must be a shell step");
        };
        assert!(run.iter().all(|argument| !argument.contains('\n')));
        assert!(
            run.iter()
                .all(|argument| !argument.contains("$(printf injected"))
        );
        let output = Command::new(&run[0])
            .args(&run[1..])
            .output()
            .expect("run serialized script");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"first line\n$(printf injected >&2)\n");
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn owned_script_rejects_unserializable_controls() {
        for script in ["printf ok\0", "printf ok\r"] {
            assert!(
                owned_bash_script_step("fixed script", script, BTreeMap::new()).is_err(),
                "script control must fail"
            );
        }
    }
}
