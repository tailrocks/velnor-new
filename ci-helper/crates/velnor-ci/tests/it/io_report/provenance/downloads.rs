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
            r#"{"restore":{"outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#
        } else {
            "{}"
        },
    )
}
