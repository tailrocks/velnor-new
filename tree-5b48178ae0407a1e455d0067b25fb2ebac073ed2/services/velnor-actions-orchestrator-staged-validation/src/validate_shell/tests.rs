use super::*;

/// One staged workflow carrying a single `run:` line, renderer-shaped.
fn staged_run(run: &str) -> Result<(tempfile::TempDir, Vec<String>), String> {
    staged_runs(std::slice::from_ref(&run))
}

/// One staged workflow carrying one `run:` line per entry, in order.
fn staged_runs(runs: &[&str]) -> Result<(tempfile::TempDir, Vec<String>), String> {
    let mut yaml = String::from("jobs:\n  a:\n    runs-on: ubuntu-26.04\n    steps:\n");
    for (index, run) in runs.iter().enumerate() {
        use std::fmt::Write as _;
        writeln!(yaml, "      - name: t{index}\n        run: {run}")
            .map_err(|err| err.to_string())?;
    }
    staged_workflow(&yaml)
}

fn staged_workflow(yaml: &str) -> Result<(tempfile::TempDir, Vec<String>), String> {
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let rel = ".github/workflows/t.yml".to_owned();
    std::fs::create_dir_all(dir.path().join(".github/workflows")).map_err(|err| err.to_string())?;
    std::fs::write(dir.path().join(&rel), yaml).map_err(|err| err.to_string())?;
    Ok((dir, vec![rel]))
}

#[test]
fn shellcheck_clean_passes_violation_and_block_fail() -> Result<(), String> {
    let catalog = ToolCatalog::pinned();
    let (clean, workflows) = staged_run("echo \"hi\"")?;
    run_shellcheck_bodies(&catalog, clean.path(), &workflows).map_err(|err| err.to_string())?;
    // Error-level SC2070 trips the `-S warning` pass (see
    // `shellcheck_pure_sc2086_body_fails` for the targeted SC2086 pass).
    let (dirty, workflows) = staged_run("echo $FOO/bar && [ -n $BAZ ]")?;
    assert!(
        run_shellcheck_bodies(&catalog, dirty.path(), &workflows).is_err_and(|err| {
            matches!(&err, OrchestratorError::Validation { tool, .. } if tool == "shellcheck")
                && err.to_string().contains("SC2070")
        })
    );
    let (blocked, workflows) = staged_run("|")?;
    assert!(
        run_shellcheck_bodies(&catalog, blocked.path(), &workflows)
            .is_err_and(|err| { err.to_string().contains("run_block_scalar_unlintable") })
    );
    Ok(())
}

#[test]
fn shellcheck_pure_sc2086_body_fails() -> Result<(), String> {
    // SC2086 is info-level in shellcheck 0.11, so a pure-SC2086 body
    // passes `-S warning` and only the targeted `--include=SC2086`
    // pass fails it: deleting that pass turns this test green-on-red.
    let catalog = ToolCatalog::pinned();
    let (dirty, workflows) = staged_run("echo $FOO/bar")?;
    assert!(
        run_shellcheck_bodies(&catalog, dirty.path(), &workflows).is_err_and(|err| {
            matches!(&err, OrchestratorError::Validation { tool, .. } if tool == "shellcheck")
                && err.to_string().contains("SC2086")
        })
    );
    Ok(())
}

#[test]
fn shellcheck_batch_lints_every_body() -> Result<(), String> {
    let catalog = ToolCatalog::pinned();
    let (clean, workflows) = staged_runs(&["echo \"one\"", "echo \"two\""])?;
    run_shellcheck_bodies(&catalog, clean.path(), &workflows).map_err(|err| err.to_string())?;
    // A violation in a later body still fails the single batched run.
    let (dirty, workflows) = staged_runs(&["echo \"one\"", "echo $FOO/bar && [ -n $BAZ ]"])?;
    assert!(
        run_shellcheck_bodies(&catalog, dirty.path(), &workflows).is_err_and(|err| {
            matches!(&err, OrchestratorError::Validation { tool, .. } if tool == "shellcheck")
                && err.to_string().contains("SC2070")
        })
    );
    // All-empty bodies lint nothing and pass.
    let (empty, workflows) = staged_runs(&["\"\""])?;
    run_shellcheck_bodies(&catalog, empty.path(), &workflows).map_err(|err| err.to_string())?;
    Ok(())
}

#[test]
fn shellcheck_uses_the_resolved_bash_or_posix_dialect() -> Result<(), String> {
    let catalog = ToolCatalog::pinned();
    let yaml = |shell: &str| {
        format!(
            "jobs:\n  probe:\n    runs-on: [self-hosted, runner]\n    defaults:\n      run:\n        shell: {shell}\n    steps:\n      - name: array\n        run: \"items=(one); test \\\"${{items[0]}}\\\" = one\"\n"
        )
    };
    let (bash, workflows) = staged_workflow(&yaml("bash -e {0}"))?;
    run_shellcheck_bodies(&catalog, bash.path(), &workflows).map_err(|err| err.to_string())?;
    let (sh, workflows) = staged_workflow(&yaml("sh -e {0}"))?;
    assert!(run_shellcheck_bodies(&catalog, sh.path(), &workflows).is_err());

    let container_yaml = "jobs:\n  scale-container:\n    runs-on: [self-hosted, runner]\n    steps:\n      - name: posix\n        run: test -n \"$HOME\" && printf '%s\\n' ready\n    container: alpine:3.22\n";
    let (container, workflows) = staged_workflow(container_yaml)?;
    run_shellcheck_bodies(&catalog, container.path(), &workflows).map_err(|err| err.to_string())?;
    Ok(())
}

#[test]
fn shellcheck_version_mismatch_fails() {
    assert!(check_shellcheck_version("ShellCheck\nversion: 0.11.0\n", "0.11.0").is_ok());
    assert!(
        check_shellcheck_version("junk", "0.11.0")
            .is_err_and(|err| { err.to_string().contains("shellcheck_version_mismatch") })
    );
    assert!(
        check_shellcheck_version("version: 9.9.9\n", "0.11.0")
            .is_err_and(|err| { err.to_string().contains("shellcheck_version_mismatch") })
    );
}
