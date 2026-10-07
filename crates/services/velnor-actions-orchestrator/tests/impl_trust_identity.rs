//! Trust cases: repository identity hints versus git origin.

use velnor_actions_orchestrator::prepare;
use velnor_actions_orchestrator_core::OrchestratorError;

use crate::impl_common::{TestResult, err_of, git, make_repo};

/// Child-side identity probe for one `VELNOR_TRUST_PROBE` scenario; a no-op
/// without it. The spawner re-executes this binary per scenario with exact
/// child env, never mutating process-global env (forbidden by `unsafe_code`).
#[test]
fn trust_probe_identity_inner() -> TestResult {
    let scenario = std::env::var("VELNOR_TRUST_PROBE").unwrap_or_default();
    if scenario.is_empty() {
        return Ok(());
    }
    let repo = make_repo(
        "schema = 1\n[workflow]\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n",
    )?;
    let url = if scenario.contains("scp_canonical_origin") {
        "git@github.com:tailrocks/velnor-new.git"
    } else if scenario.contains("canonical_origin") {
        "https://github.com/tailrocks/velnor-new.git"
    } else if scenario.contains("hostile_subdomain_origin") {
        "https://github.com.evil.example/tailrocks/velnor-new.git"
    } else if scenario.contains("hostile_scp_origin") {
        "git@evil.example:tailrocks/velnor-new.git"
    } else if scenario.contains("hostile_suffix_origin") {
        "https://evil.example/tailrocks/velnor-new.git"
    } else if scenario.contains("bare_origin") {
        "tailrocks/velnor-new"
    } else {
        "https://example.com/other/repo.git"
    };
    git(&["remote", "add", "origin", url], repo.path())?;
    match scenario.as_str() {
        "canonical_hint_canonical_origin"
        | "canonical_upper_hint_canonical_origin"
        | "no_hint_canonical_origin"
        | "no_hint_scp_canonical_origin" => {
            prepare(repo.path())?;
        }
        name => {
            let err = err_of(prepare(repo.path()), name)?;
            assert!(
                matches!(err, OrchestratorError::IdentityRejected { .. }),
                "got {err}"
            );
            let want = if name.contains("evil_hint") {
                "mismatch"
            } else {
                "velnor_policy_requires"
            };
            assert!(err.to_string().contains(want), "got {err}");
        }
    }
    Ok(())
}

#[test]
fn identity_hint_scenarios() -> TestResult {
    let cases = [
        ("canonical_hint_fork_origin", Some("tailrocks/velnor-new")),
        ("evil_hint_canonical_origin", Some("evil/fork")),
        ("evil_hint_fork_origin", Some("evil/fork")),
        (
            "canonical_hint_canonical_origin",
            Some("tailrocks/velnor-new"),
        ),
        (
            "canonical_upper_hint_canonical_origin",
            Some("Tailrocks/Velnor-New"),
        ),
        ("no_hint_canonical_origin", None),
        ("no_hint_scp_canonical_origin", None),
        ("no_hint_hostile_suffix_origin", None),
        ("no_hint_hostile_scp_origin", None),
        ("no_hint_hostile_subdomain_origin", None),
        ("no_hint_bare_origin", None),
    ];
    for (probe, hint) in cases {
        let mut child = std::process::Command::new(std::env::current_exe()?);
        child.env("VELNOR_TRUST_PROBE", probe);
        if let Some(value) = hint {
            child.env("GITHUB_REPOSITORY", value);
        } else {
            child.env_remove("GITHUB_REPOSITORY");
        }
        let output = child.arg("trust_probe_identity_inner").output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{probe}: {stdout}{stderr}");
        assert!(stdout.contains("1 passed"), "{probe}: {stdout}");
    }
    Ok(())
}
