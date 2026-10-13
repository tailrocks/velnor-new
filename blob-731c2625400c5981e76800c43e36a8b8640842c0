//! Configuration validation and repository identity integration cases.

use super::*;

#[test]
fn runner_label_exact_match_at_prepare() -> TestResult {
    for label in ["ubuntu-latest", "ubuntu-24.04-latest", "ubuntu-99.04"] {
        let repo = make_repo(&format!(
            "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\nrunner_label = \"{label}\"\n"
        ))?;
        let err = err_of(prepare(repo.path()), "label rejected")?;
        assert!(
            err.to_string().contains("workflow.runner_label"),
            "{label}: got {err}"
        );
    }
    let repo = make_repo(
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\nrunner_label = \"ubuntu-22.04\"\n",
    )?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.runner_label, "ubuntu-22.04");
    assert_eq!(prep.runner_selection, RunnerSelection::ConfigOverride);
    // Gate 8: `-arm` labels have no supported release target, so consumer
    // generation fails rather than embedding a wrong-architecture asset.
    for label in ["ubuntu-24.04-arm", "ubuntu-26.04-arm"] {
        let repo = make_repo(&format!(
            "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\nrunner_label = \"{label}\"\n"
        ))?;
        let err = err_of(prepare(repo.path()), "arm label rejected")?;
        assert!(
            err.to_string().contains("unsupported_target_for_runner"),
            "{label}: got {err}"
        );
    }
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.runner_label, "ubuntu-26.04");
    assert_eq!(prep.runner_selection, RunnerSelection::LatestDefault);
    Ok(())
}

#[test]
fn uppercase_rust_name_rejected_with_key_path() -> TestResult {
    let repo = make_repo(
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks.rust]\nconfigurations = [{ name = \"Default\", features = [], target = \"host\" }]\n",
    )?;
    let err = err_of(prepare(repo.path()), "uppercase rejected")?;
    assert!(
        err.to_string().contains("stacks.rust.configurations.name"),
        "got {err}"
    );
    Ok(())
}

#[test]
fn branch_failure_hints_default_branch_setting() -> TestResult {
    let repo = make_repo("schema = 1\n")?;
    let err = err_of(prepare(repo.path()), "branch unresolvable")?;
    assert!(
        matches!(err, OrchestratorError::DefaultBranch { .. }),
        "got {err}"
    );
    assert!(
        err.to_string().contains("set workflow.default_branch"),
        "got {err}"
    );
    Ok(())
}

#[test]
fn identity_ignores_decoy_lines_and_remotes() -> TestResult {
    without_ambient_identity("identity_ignores_decoy_lines_and_remotes", || {
        let config = "schema = 1\n[workflow]\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n";
        // Decoy: identity in another remote, in a non-url key, and in comments.
        let repo = make_repo(config)?;
        let git_config = repo.path().join(".git/config");
        let mut text = fs::read_to_string(&git_config)?;
        text.push_str(
        "[remote \"upstream\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n[remote \"origin\"]\n\turl = https://example.com/other/repo.git\n\tpushurl = https://github.com/tailrocks/velnor-new.git\n# tailrocks/velnor-new\n",
    );
        fs::write(&git_config, text)?;
        let err = err_of(prepare(repo.path()), "decoys rejected")?;
        assert!(
            matches!(err, OrchestratorError::IdentityRejected { .. }),
            "got {err}"
        );
        // Positive control: origin url grants the identity.
        let repo = make_repo(config)?;
        let git_config = repo.path().join(".git/config");
        let mut text = fs::read_to_string(&git_config)?;
        text.push_str("[remote \"origin\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n");
        fs::write(&git_config, text)?;
        prepare(repo.path())?;
        Ok(())
    })
}
