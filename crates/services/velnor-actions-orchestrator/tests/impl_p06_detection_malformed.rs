//! P06 detection fail-closed cases: malformed configs, section wrappers.
//!
//! Split from `impl_p06_detection` to hold the 400-line size gate; wired
//! into `velnor_orchestrator` by the parent with one `mod` line. Uses
//! `crate::impl_common` fixtures.

use std::fs;
use std::path::Path;

use velnor_actions_orchestrator::prepare;

use crate::impl_common::{TestResult, config_with_branch, err_of, make_repo, snapshot};

/// Write `content` to `relative` under `root`, creating parents.
fn write_file(root: &Path, relative: &str, content: &str) -> TestResult {
    let target = root.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, content)?;
    Ok(())
}

#[test]
fn malformed_nextest_config_fails_closed() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_file(root, ".config/nextest.toml", "[profile.ci\nretries = \n")?;
    let before = snapshot(root)?;
    let err = err_of(prepare(root).map(|_| ()), "malformed nextest")?;
    assert!(
        err.to_string().contains("nextest_config_invalid"),
        "diagnostic: {err}"
    );
    assert_eq!(snapshot(root)?, before, ".github untouched");
    Ok(())
}

#[test]
fn malformed_mise_wrapper_fails_closed() -> TestResult {
    for name in ["mise.toml", ".mise.toml"] {
        for content in ["[tools\nrust = \n", "[wrappers.cargo]\ncommand = true\n"] {
            let repo = make_repo(config_with_branch())?;
            let root = repo.path();
            write_file(root, name, content)?;
            let before = snapshot(root)?;
            let err = err_of(prepare(root).map(|_| ()), "malformed wrapper")?;
            assert!(
                err.to_string().contains("wrapper_invalid"),
                "diagnostic: {err}"
            );
            assert!(err.to_string().contains(name), "file named: {err}");
            assert_eq!(snapshot(root)?, before, ".github untouched");
        }
    }
    Ok(())
}

#[test]
fn valid_section_wrapper_selects_mbx() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_file(
        root,
        "mise.toml",
        "[wrappers.cargo]\ncommand = \"mbx\"\n[wrappers.cargo.env]\nMBX_CARGO_SHIM_MODE = \"1\"\n",
    )?;
    let prep = prepare(root)?;
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(workspace.profile.compile_driver.as_str(), "mbx");
    assert_eq!(workspace.profile.driver_source.as_str(), "detected");
    assert_eq!(workspace.profile.evidence.len(), 1);
    let sighting = &workspace.profile.evidence[0];
    assert_eq!(sighting.path, "mise.toml");
    assert_eq!(sighting.line, 2);
    assert!(
        sighting
            .command_or_setting
            .contains("wrappers.cargo.command")
    );
    assert!(workspace.findings.is_empty());
    Ok(())
}
