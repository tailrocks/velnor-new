//! Source-bound local taps and strict online audits through isolated Homebrew.

use crate::OrchestratorError;
use velnor_actions_contract::FileIndex;
use velnor_actions_contract::config::{WorkloadConfig, WorkloadKind};
use velnor_actions_mise::PinnedTool;

#[path = "workloads_homebrew_targets.rs"]
mod targets;

/// Every Homebrew input participates; absence of a Brewfile is permitted.
pub(crate) fn inputs(index: &FileIndex) -> Vec<String> {
    velnor_actions_native::homebrew::audit::input_paths(index.files())
}

#[cfg(test)]
fn is_input(path: &str) -> bool {
    !velnor_actions_native::homebrew::audit::input_paths(&[path.to_owned()]).is_empty()
}

#[cfg(test)]
fn is_cask(path: &str) -> bool {
    path.starts_with("Casks/") && path.ends_with(".rb")
}

pub(crate) fn validate_evidence(
    workload: &WorkloadConfig,
    index: &FileIndex,
) -> Result<(), OrchestratorError> {
    if workload.kind != WorkloadKind::HomebrewAudit {
        return Ok(());
    }
    if workload.root.as_str() != "." {
        return Err(crate::internal::internal(
            "homebrew_requires_repository_root",
        ));
    }
    if inputs(index).is_empty() {
        return Err(crate::internal::internal("homebrew_tap_evidence_missing"));
    }
    Ok(())
}

pub(crate) const fn kind_id(kind: WorkloadKind) -> &'static str {
    if matches!(kind, WorkloadKind::HomebrewAudit) {
        "homebrew_audit"
    } else {
        ""
    }
}

/// Homebrew's pinned source owns its checksum-verified portable Ruby bootstrap.
pub(crate) fn tools(_kind: WorkloadKind) -> Vec<PinnedTool> {
    Vec::new()
}

/// The audit identity binds both Brew source and checksum-verified Ruby blobs.
pub(crate) fn toolchain_recipe() -> Result<String, OrchestratorError> {
    Ok(source_identity()?.recipe())
}

fn source_identity()
-> Result<velnor_actions_native::homebrew::audit::BrewSourceIdentity, OrchestratorError> {
    use velnor_actions_mise::catalog::homebrew;
    velnor_actions_native::homebrew::audit::BrewSourceIdentity::reviewed(
        homebrew::VERSION,
        homebrew::SOURCE_SHA,
        homebrew::PORTABLE_RUBY_VERSION,
        homebrew::PORTABLE_RUBY_X86_64_LINUX_SHA256,
        homebrew::PORTABLE_RUBY_ARM64_LINUX_SHA256,
    )
    .map_err(|error| OrchestratorError::Contract {
        problem: error.to_string(),
    })
}

/// The tap identity enters argv before planning; runtime environment cannot pick it.
pub(crate) fn phases(
    workload: &WorkloadConfig,
    index: &FileIndex,
) -> Result<Vec<(&'static str, Vec<String>)>, OrchestratorError> {
    if workload.kind != WorkloadKind::HomebrewAudit {
        return Ok(Vec::new());
    }
    validate_evidence(workload, index)?;
    let repository = source_repository(index)?;
    let tap = velnor_actions_native::homebrew::audit::TapIdentity::from_repository(&repository)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    let targets = targets::admit(index, &tap)?;
    let mut preparation = vec!["homebrew-preparation".to_owned()];
    preparation.extend(
        velnor_actions_native::homebrew::preparation::preparation_arguments(
            &source_identity()?,
            &tap,
            !targets.casks().is_empty(),
        ),
    );
    let mut phases = vec![
        ("homebrew-tap-local", preparation),
        ("homebrew-audit", audit_argv()),
    ];
    // Bare audit is already present; the pure owner supplies every scoped witness.
    for (phase, arguments) in velnor_actions_native::homebrew::audit::audit_arguments(&targets)
        .into_iter()
        .skip(1)
    {
        let mut argv = audit_prefix();
        argv.extend(arguments);
        phases.push((phase, argv));
    }
    Ok(phases)
}

/// Bind a closed logical preparation payload to the native owner's full bytes.
pub(crate) fn preparation_source(
    payload: &[String],
    version: &str,
) -> Result<(velnor_actions_native::SupportBundle, Vec<String>), OrchestratorError> {
    let Some((operation, arguments)) = payload.split_first() else {
        return Err(crate::internal::internal(
            "homebrew_preparation_payload_missing",
        ));
    };
    if operation != "homebrew-preparation" {
        return Err(crate::internal::internal(
            "homebrew_preparation_operation_invalid",
        ));
    }
    let source = velnor_actions_native::homebrew::preparation::preparation_for_arguments(
        &source_identity()?,
        arguments,
        version,
    )
    .map_err(|error| OrchestratorError::Contract {
        problem: error.to_string(),
    })?;
    Ok((source, arguments.to_vec()))
}

fn audit_argv() -> Vec<String> {
    let mut argv = audit_prefix();
    argv.extend([
        "audit".to_owned(),
        "--strict".to_owned(),
        "--online".to_owned(),
    ]);
    argv
}

fn audit_prefix() -> Vec<String> {
    vec![
        "env".to_owned(),
        "-i".to_owned(),
        "PATH=/usr/bin:/bin:/usr/sbin:/sbin".to_owned(),
        "PWD=${{ github.workspace }}".to_owned(),
        "HOME=${{ runner.temp }}/velnor/homebrew/home".to_owned(),
        "HOMEBREW_CACHE=${{ runner.temp }}/velnor/homebrew/cache".to_owned(),
        "HOMEBREW_LOGS=${{ runner.temp }}/velnor/homebrew/logs".to_owned(),
        "HOMEBREW_NO_AUTO_UPDATE=1".to_owned(),
        "HOMEBREW_FORCE_VENDOR_RUBY=1".to_owned(),
        "HOMEBREW_NO_ANALYTICS=1".to_owned(),
        "${{ runner.temp }}/velnor/homebrew/bin/brew".to_owned(),
    ]
}

fn source_repository(index: &FileIndex) -> Result<String, OrchestratorError> {
    use crate::cover_baseline::{provenance_check, provenance_resolve};
    let origin = provenance_check::repository_slug_from_origin(index.root());
    let request = match std::env::var(crate::origin::GITHUB_REPOSITORY_ENV) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(crate::internal::internal("homebrew_repository_not_utf8"));
        }
    };
    let resolved =
        provenance_resolve::resolve_expected_repository(origin.as_deref(), request.as_deref());
    if resolved.conflict {
        return Err(crate::internal::internal("homebrew_repository_conflict"));
    }
    resolved
        .slug
        .ok_or_else(|| crate::internal::internal("homebrew_repository_unanchored"))
}

#[cfg(test)]
fn tap_identity(repository: &str) -> Result<(String, String), OrchestratorError> {
    let tap = velnor_actions_native::homebrew::audit::TapIdentity::from_repository(repository)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    Ok((tap.owner().to_owned(), tap.name().to_owned()))
}

#[cfg(test)]
fn setup_script(has_casks: bool) -> String {
    velnor_actions_native::homebrew::preparation::preparation_body(has_casks)
}

pub(crate) fn rank(phase: &str) -> Option<u32> {
    match phase {
        "homebrew-tap-local" => Some(0),
        "homebrew-audit" => Some(1),
        "homebrew-audit-formula" => Some(2),
        "homebrew-audit-cask" => Some(3),
        _ => None,
    }
}

pub(crate) fn step_name(phase: &str) -> Option<&'static str> {
    match phase {
        "homebrew-tap-local" => Some("Prepare isolated local Homebrew tap"),
        "homebrew-audit" => Some("Strict online Homebrew audit"),
        "homebrew-audit-formula" => Some("Audit indexed checkout formulae"),
        "homebrew-audit-cask" => Some("Audit indexed checkout casks"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_identity_cannot_inject_shell_or_escape_tap_directory() {
        assert_eq!(
            tap_identity("tailrocks/homebrew-velnor").expect("reviewed tap"),
            ("tailrocks".to_owned(), "velnor".to_owned())
        );
        for repository in [
            "tailrocks/velnor",
            "../homebrew-velnor",
            "owner/homebrew-../other",
            "owner/homebrew--option",
            "owner/homebrew-$(cmd)",
            "owner/homebrew-x\nexport TOKEN=value",
            "owner/homebrew-",
        ] {
            assert!(tap_identity(repository).is_err(), "{repository}");
        }
    }

    #[test]
    fn preparation_is_fixed_and_does_not_execute_repository_tasks() {
        let script = setup_script(false);
        velnor_actions_workflow_renderer::validate_command_argv(&[
            "sh".to_owned(),
            "-c".to_owned(),
            script.clone(),
        ])
        .expect("fixed renderer vector");
        assert!(script.contains("test \"$source_sha\" = \"$3\""));
        assert!(script.contains("mktemp -d /tmp/vb.XXXXXXXX"));
        assert!(script.contains("test ! -e \"$alias\""));
        assert!(script.contains("GIT_CONFIG_GLOBAL=/dev/null"));
        for denied in ["mise run", "scripts/", "rm -rf", "untap", "sudo", "eval"] {
            assert!(!script.contains(denied), "{denied}");
        }
        assert!(setup_script(true).contains("homebrew_cask_plutil_unavailable"));
        assert!(!script.contains("homebrew_cask_plutil_unavailable"));
    }

    #[test]
    fn watch_inputs_cover_formulae_casks_and_brewfile() {
        for path in ["Brewfile", "Formula/tool.rb", "Casks/tool.rb"] {
            assert!(is_input(path));
        }
        assert!(!is_input("nested/Formula/tool.rb"));
        assert!(!is_input("Formula.md"));
        assert!(is_cask("Casks/tool.rb"));
        assert!(!is_cask("Casks/README.md"));
        assert!(rank("homebrew-tap-local") < rank("homebrew-audit"));
        assert_eq!(rank("unreviewed"), None);
        assert_eq!(step_name("unreviewed"), None);
        assert!(tools(WorkloadKind::HomebrewAudit).is_empty());
    }
}
