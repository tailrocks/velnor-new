//! Audit plain installs and exact source-owner Rust install invocations.

use velnor_actions_contract::{Step, StepKind};

use super::{MiseVectors, classify_mise_vectors};

/// Specs from a Prepare step's plain argv; a Prepare step always
/// installs, so a missing or unclassifiable vector blocks as a
/// generator bug.
pub(super) fn prepare_specs(
    id: &str,
    step: &Step,
    blocking: &mut Vec<String>,
) -> Option<Vec<String>> {
    let install;
    let argv = match &step.kind {
        StepKind::Shell { run, .. } => run.as_slice(),
        StepKind::SourceBoundHelper { invocation, env } => {
            let version = env!("CARGO_PKG_VERSION");
            if velnor_actions_mise::catalog::rust_prepare::record_for_invocation(
                invocation, env, version,
            )
            .is_err()
            {
                blocking.push(format!(
                    "unauditable_prepare_argv:{id}:invalid_source_owner"
                ));
                return None;
            }
            install =
                velnor_actions_mise::catalog::rust_prepare::install_argv(invocation, version)?;
            install.as_slice()
        }
        _ => {
            blocking.push(format!("unauditable_prepare_argv:{id}:not_an_install_step"));
            return None;
        }
    };
    let tokens: Vec<&str> = argv.iter().map(String::as_str).collect();
    match classify_mise_vectors(&tokens) {
        MiseVectors::Install(specs) => Some(specs),
        MiseVectors::NotInstall => {
            blocking.push(format!("unauditable_prepare_argv:{id}:no_install"));
            None
        }
        MiseVectors::Unclassifiable(detail) => {
            blocking.push(format!("unauditable_prepare_argv:{id}:{detail}"));
            None
        }
    }
}
