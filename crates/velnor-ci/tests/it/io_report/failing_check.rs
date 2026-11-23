use super::*;

#[test]
fn a_failing_check_prints_the_restore_warning_on_stdout_and_keeps_it_out_of_the_error() -> ResultTest
{
    let repo = Repo::new("report-check-failing");
    let mut value = crate::report::lane();
    value["invocations"][0]["restored_manifest"] = json!("absent");
    value["invocations"][0]["compilations"]
        .as_array_mut()
        .ok_or("compilations is not an array")?
        .push(json!({
            "package": "tokio",
            "version": "1.0.0",
            "workspace": false,
            "unchanged": false,
            "dependencies": [],
            "restored": true
        }));
    fs::create_dir_all(repo.root().join("artifacts/rust"))?;
    fs::write(
        repo.root().join("artifacts/rust/ci-metrics.json"),
        value.to_string(),
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["report", "--check"])
        .arg(repo.root().join("artifacts"))
        .env("VELNOR_REPORT_NEEDS", r#"{"rust":{"result":"success"}}"#)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        stdout
            .lines()
            .any(|line| { line.starts_with("::warning::") && line.contains("serde v1.0.0") }),
        "{stdout}"
    );
    assert!(
        stderr.contains("tokio v1.0.0 compiled despite usable restored inputs"),
        "{stderr}"
    );
    assert!(!stderr.contains("::warning::"), "{stderr}");
    Ok(())
}
