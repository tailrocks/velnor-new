//! Strict direct command recognition for rejecting unselected MBX.
use super::is_exact_mbx_version;
use velnor_actions_contract::{Step, StepKind};

/// Direct compiler/install vector presence; opaque helpers are checked by registry.
pub(super) fn uses_mbx_tool(step: &Step) -> bool {
    match &step.kind {
        StepKind::Shell { run, .. } => {
            executes_mbx(step)
                || direct_mise_selectors(run)
                    .iter()
                    .any(|word| is_mbx_selector(word))
        }
        StepKind::SourceBoundHelper { invocation, .. } => invocation
            .installed_selectors()
            .iter()
            .any(|selector| is_mbx_selector(selector)),
        _ => false,
    }
}

/// Transport or installation alone cannot prove selected compiler execution.
pub(super) fn executes_mbx(step: &Step) -> bool {
    let StepKind::Shell { run, .. } = &step.kind else {
        return false;
    };
    let run = &run[crate::toolchain_env::unset_prefix_len(run)..];
    if run.first().is_some_and(|program| program == "mbx") {
        return true;
    }
    let Some((operation, tail)) = direct_mise_operation(run) else {
        return false;
    };
    operation == "exec"
        && tail
            .iter()
            .position(|arg| arg == "--")
            .and_then(|at| tail.get(at + 1))
            .is_some_and(|program| program == "mbx")
}

/// Recognize only the generator's fixed direct Mise vector, never shell text.
fn direct_mise_operation(run: &[String]) -> Option<(&str, &[String])> {
    let run = &run[crate::toolchain_env::unset_prefix_len(run)..];
    if run.first().is_none_or(|program| program != "mise") {
        return None;
    }
    let at = run.iter().skip(1).position(|arg| !arg.starts_with("--"))? + 1;
    if run[1..at]
        .iter()
        .any(|arg| !matches!(arg.as_str(), "--no-config" | "--no-env" | "--no-hooks"))
    {
        return None;
    }
    matches!(run[at].as_str(), "install" | "exec").then_some((run[at].as_str(), &run[at + 1..]))
}

fn direct_mise_selectors(run: &[String]) -> &[String] {
    let Some((operation, tail)) = direct_mise_operation(run) else {
        return &[];
    };
    if operation == "install" {
        tail
    } else {
        tail.iter()
            .position(|arg| arg == "--")
            .map_or(&[], |at| &tail[..at])
    }
}

fn is_mbx_selector(selector: &str) -> bool {
    ["mr-boxington@", "github:jdx/mr-boxington@"]
        .iter()
        .any(|prefix| {
            selector
                .strip_prefix(prefix)
                .is_some_and(is_exact_mbx_version)
        })
}
