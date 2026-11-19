// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn quiet_archive_changes_count_on_cold_and_exact_source_hits() -> super::super::ResultTest {
    let repo = fixture("report-quiet-download")?;
    let home = repo.root().join("cargo-home");
    fs::create_dir_all(&home)?;
    let script = repo.root().join("cargo-fixture");
    fs::write(
        &script,
        "#!/bin/sh\nmkdir -p \"$CARGO_HOME/registry/cache/index\"\nprintf archive > \"$CARGO_HOME/registry/cache/index/demo-0.1.0.crate\"\n",
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--"])
        .arg(script)
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("CARGO_HOME", home)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    for exact in [false, true] {
        let metrics = source_report(&repo, exact)?;
        assert_eq!(metrics.invocations[0].downloads, ["index/demo-0.1.0.crate"]);
        assert_eq!(metrics.downloaded, 1);
        assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
        assert_eq!(velnor_ci::report::check(&[metrics]).is_err(), exact);
    }
    Ok(())
}

#[test]
fn quiet_exact_source_hit_requires_available_archive_observation() -> super::super::ResultTest {
    let repo = fixture("report-quiet-download-unknown")?;
    for archives in [json!([]), Value::Null] {
        let mut invocation = crate::report::lane()["invocations"][0].clone();
        invocation["root"] = json!(fs::canonicalize(repo.root())?);
        invocation["compilations"] = json!([]);
        invocation["artifacts"] = json!([]);
        invocation["progress"] = json!([]);
        invocation["verified_download_archives"] = archives.clone();
        fs::write(repo.root().join(INVOCATIONS), format!("{invocation}\n"))?;
        let metrics = source_report(&repo, true)?;
        assert_eq!(metrics.downloaded, 0);
        assert_eq!(metrics.invocations[0].downloads, [] as [String; 0]);
        assert_eq!(
            metrics.verification_errors.is_empty(),
            !archives.is_null(),
            "{metrics:?}"
        );
        assert_eq!(
            velnor_ci::report::check(&[metrics]).is_err(),
            archives.is_null()
        );
    }
    Ok(())
}

fn source_report(repo: &Repo, exact: bool) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let caches =
        json!([{"family":"sources","root":".","restore_step":"restore","save_step":"save"}]);
    super::super::report(
        repo,
        &caches,
        if exact {
            r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#
        } else {
            r#"{"restore":{"outcome":"skipped","outputs":{}}}"#
        },
    )
}

#[test]
fn actual_git_database_fetch_is_reported_on_an_exact_sources_hit() -> super::super::ResultTest {
    let repo = fixture("report-git-fetch")?;
    let source = Repo::new("report-git-source");
    source.write("source.txt", "source contents\n");
    source.commit();
    let script = "#!/bin/sh\nset -eu\nmkdir -p \"$CARGO_HOME/git/db\"\ngit clone --bare --no-hardlinks \"$1\" \"$CARGO_HOME/git/db/source\"\nprintf '    Updating git repository source\\n' >&2\n";
    run_git(&repo, script, source.root())?;
    let metrics = source_report(&repo, true)?;
    assert_eq!(metrics.downloaded, 0);
    assert_eq!(
        metrics.invocations[0].verified_git_downloads,
        Some(vec!["git/db/source".to_owned()])
    );
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    assert!(velnor_ci::report::check(&[metrics]).is_err());
    Ok(())
}

#[test]
fn spurious_git_progress_does_not_claim_a_fetch_without_database_changes()
-> super::super::ResultTest {
    let repo = fixture("report-git-spoof")?;
    run_git(
        &repo,
        "#!/bin/sh\nprintf '    Updating git repository source\\n' >&2\n",
        repo.root(),
    )?;
    let metrics = source_report(&repo, true)?;
    assert_eq!(metrics.downloaded, 0);
    assert_eq!(
        metrics.invocations[0].verified_git_downloads,
        Some(Vec::new())
    );
    assert!(velnor_ci::report::check(&[metrics]).is_ok());
    Ok(())
}

#[test]
fn unreadable_git_observation_cannot_report_an_exact_sources_hit_clean() -> super::super::ResultTest
{
    let repo = fixture("report-git-unknown")?;
    let home = repo.root().join("cargo-home");
    fs::create_dir_all(home.join("git"))?;
    fs::write(home.join("git/db"), "not a directory")?;
    run_git(&repo, "#!/bin/sh\ntrue\n", repo.root())?;
    let metrics = source_report(&repo, true)?;
    assert_eq!(metrics.invocations[0].verified_git_downloads, None);
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|error| error.contains("Git database evidence"))
    );
    assert!(velnor_ci::report::check(&[metrics]).is_err());
    Ok(())
}

fn run_git(repo: &Repo, body: &str, source: &std::path::Path) -> super::super::ResultTest {
    let script = repo.root().join("cargo-fixture");
    fs::write(&script, body)?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--"])
        .arg(script)
        .arg(source)
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("CARGO_HOME", repo.root().join("cargo-home"))
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(())
}

#[test]
fn missing_cargo_home_is_known_empty_only_for_commands_that_cannot_fetch()
-> super::super::ResultTest {
    let repo = Repo::new("report-git-missing-home");
    let cargo = repo.root().join("cargo");
    fs::write(&cargo, "#!/bin/sh\ntrue\n")?;
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))?;
    for (command, expected) in [("fmt", json!([])), ("fetch", Value::Null)] {
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["run", "--"])
            .arg(&cargo)
            .arg(command)
            .current_dir(repo.root())
            .env("RUNNER_TEMP", repo.root())
            .env_remove("CARGO_HOME")
            .env_remove("HOME")
            .output()?;
        assert!(output.status.success(), "{output:?}");
        let records = fs::read_to_string(repo.root().join(INVOCATIONS))?;
        let last: Value =
            serde_json::from_str(records.lines().last().ok_or("missing invocation")?)?;
        assert_eq!(last["verified_git_downloads"], expected, "{command}");
    }
    Ok(())
}
