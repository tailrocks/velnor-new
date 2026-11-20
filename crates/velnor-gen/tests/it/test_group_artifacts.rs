// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_rust::detect;

use crate::facts::TestResult;
use crate::test_group_partitions::{HELPER, group, refused, rendered_with};
use crate::test_groups::{job, run_of, steps, view_with};

const UPLOAD: &str = "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9";

fn artifacts(entries: &str) -> String {
    group(&format!("artifacts = [{entries}]\n"))
}

fn entry(body: &str) -> String {
    format!("{{ name = 'database-failures', paths = ['junit.xml'], {body} }}")
}

fn uploads(job: &Value) -> Vec<&Value> {
    steps(job)
        .iter()
        .filter(|step| {
            step["uses"] == UPLOAD
                && step["with"]["path"]
                    .as_str()
                    .is_some_and(|path| path.contains("velnor-artifacts/"))
        })
        .collect()
}

#[test]
fn artifacts_parse_with_their_defaults() -> TestResult {
    let config = artifacts(&entry("when = 'failure'"));
    let facts = detect(&view_with(&["Cargo.toml"], &config, HELPER)?)?;
    let artifact = &facts.config.test_groups[0].artifacts[0];
    assert_eq!(artifact.name, "database-failures");
    assert_eq!(artifact.retention_days, 3);
    assert_eq!(artifact.paths.len(), 1);
    let facts = detect(&view_with(
        &["Cargo.toml"],
        &artifacts(&entry("when = 'always', retention_days = 90")),
        HELPER,
    )?)?;
    assert_eq!(facts.config.test_groups[0].artifacts[0].retention_days, 90);
    let plain = detect(&view_with(&["Cargo.toml"], &group(""), HELPER)?)?;
    assert_eq!(plain.config.test_groups[0].artifacts, []);
    Ok(())
}

#[test]
fn every_bad_artifact_key_is_refused_with_its_name() -> TestResult {
    let named =
        |name: &str| format!("{{ name = '{name}', paths = ['junit.xml'], when = 'failure' }}");
    let long = "n".repeat(61);
    for (config, mention) in [
        (
            artifacts(&entry("when = 'sometimes'")),
            "test_groups.artifacts.when",
        ),
        (artifacts("{ name = 'a', paths = ['x'] }"), "when"),
        (
            artifacts(&entry("when = 'failure', retention_days = 0")),
            "retention_days",
        ),
        (
            artifacts(&entry("when = 'failure', retention_days = 91")),
            "retention_days",
        ),
        (
            artifacts(&entry("when = 'failure', color = 'red'")),
            "color",
        ),
        (artifacts("{ name = 'a', when = 'failure' }"), "paths"),
        (
            artifacts("{ name = 'a', paths = [], when = 'failure' }"),
            "test_groups.artifacts.paths",
        ),
        (artifacts(&named("")), "test_groups.artifacts.name"),
        (artifacts(&named("has space")), "test_groups.artifacts.name"),
        (
            artifacts(&named("slash/name")),
            "test_groups.artifacts.name",
        ),
        (artifacts(&named(&long)), "test_groups.artifacts.name"),
        (
            artifacts(&named("nextest-archive")),
            "test_groups.artifacts.name",
        ),
        (
            artifacts(&named("velnor-cargo-log")),
            "test_groups.artifacts.name",
        ),
        (group("artifacts = []\n"), "test_groups.artifacts"),
    ] {
        let message = refused(&config, HELPER)?;
        assert!(message.contains(mention), "{config}\n{message}");
    }
    Ok(())
}

#[test]
fn a_path_outside_the_workspace_or_into_secrets_is_refused() -> TestResult {
    for path in [
        "/etc/passwd",
        "../outside.xml",
        "a/../../b",
        "a/..",
        "~/x",
        "${{ github.workspace }}/x",
        "target",
        "target/",
        "app/target/",
        "target/**",
        "target/debug",
        "target/release/**",
        "**/*.xml",
        "**/junit.xml",
        "**",
        "*",
        ".",
        "./junit.xml",
        "a//b",
        ".git/config",
        "app/.git/hooks",
        ".env",
        "app/.env.local",
        "certs/server.pem",
        "deploy.key",
        "keys/app.json",
        "id_rsa",
        ".cargo/credentials.toml",
        ".cargo/credentials",
        "secrets/x.txt",
        ".netrc",
        "a\\b",
        "a\nb",
        "a b",
        "!junit.xml",
        "--help",
        "-x/junit.xml",
        ".reports/junit.xml",
        "reports/.junit.xml",
    ] {
        let config = artifacts(&format!(
            "{{ name = 'a', paths = [\"\"\"{path}\"\"\"], when = 'failure' }}"
        ));
        let message = refused(&config, HELPER)?;
        assert!(
            message.contains("test_groups.artifacts.paths"),
            "{path:?}: {message}"
        );
    }
    for path in [
        "junit.xml",
        "app/target/nextest/ci/junit.xml",
        "app/tests/snapshots/",
        "logs/**/*.log",
        "target/nextest/ci/*.xml",
    ] {
        let config = artifacts(&format!(
            "{{ name = 'a', paths = ['{path}'], when = 'failure' }}"
        ));
        rendered_with(&config, HELPER)?;
    }
    Ok(())
}

#[test]
fn artifact_names_must_be_unique_across_groups_and_partitions() -> TestResult {
    let twice = format!(
        "{}\n[[test_groups]]\nname = 'shell'\nfilter = 'all()'\nartifacts = [{}]\n",
        artifacts(&entry("when = 'failure'")),
        entry("when = 'always'")
    );
    let message = refused(&twice, HELPER)?;
    assert!(message.contains("database-failures"), "{message}");
    let split = format!(
        "{}\n[[test_groups]]\nname = 'shell'\nfilter = 'all()'\nartifacts = [{{ name = 'database-failures-1', paths = ['x.xml'], when = 'always' }}]\n",
        group(&format!(
            "partitions = 2\nartifacts = [{}]\n",
            entry("when = 'failure'")
        ))
    );
    let message = refused(&split, HELPER)?;
    assert!(message.contains("database-failures-1"), "{message}");
    Ok(())
}

#[test]
fn a_failure_artifact_is_uploaded_after_the_tests_under_the_default_token_only() -> TestResult {
    let config = group(
        "artifacts = [{ name = 'database-failures', paths = ['app/target/nextest/ci/junit.xml', 'app/tests/snapshots/'], when = 'failure', retention_days = 5 }]\n",
    );
    let workflow = rendered_with(&config, HELPER)?;
    let job = job(&workflow, "rust-test-database")?;
    let upload = uploads(job);
    assert_eq!(upload.len(), 1, "{upload:?}");
    let upload = upload[0];
    assert_eq!(
        upload["if"], "${{ (steps.artifact_stage_0.outcome == 'success') && (failure()) }}",
        "{upload}"
    );
    assert_eq!(upload["with"]["name"], "database-failures");
    assert_eq!(
        upload["with"]["path"],
        "${{ runner.temp }}/velnor-artifacts/0"
    );
    assert_eq!(upload["with"]["retention-days"], 5);
    assert_eq!(upload["with"]["if-no-files-found"], "warn");
    assert!(upload["env"].is_null(), "{upload}");
    let keys: Vec<&str> = upload["with"]
        .as_object()
        .map(|with| with.keys().map(String::as_str).collect())
        .unwrap_or_default();
    assert!(keys.iter().all(|key| !key.contains("token")), "{keys:?}");
    let all = steps(job);
    let test = all
        .iter()
        .position(|s| s["name"] == "Test")
        .ok_or("no test")?;
    let at = all
        .iter()
        .position(|s| s["with"]["name"] == "database-failures")
        .ok_or("no upload")?;
    assert!(test < at, "the upload runs after the tests");
    let plain = rendered_with(&group(""), HELPER)?;
    assert_eq!(
        job["permissions"],
        plain["jobs"]["rust-test-database"]["permissions"]
    );
    assert_eq!(job["permissions"]["contents"], "read");
    assert!(job["permissions"]["id-token"].is_null() && job["permissions"]["actions"] != "write");
    let build = job_uses(&workflow, "rust-test")?;
    assert!(build.contains(&UPLOAD.to_owned()), "{build:?}");
    assert!(run_of(job, "Test").is_ok());
    Ok(())
}

fn job_uses(workflow: &Value, id: &str) -> Result<Vec<String>, String> {
    Ok(steps(job(workflow, id)?)
        .iter()
        .filter_map(|step| step["uses"].as_str().map(str::to_owned))
        .collect())
}

#[test]
fn an_always_artifact_renders_always_and_each_partition_gets_its_own_name() -> TestResult {
    let config = group(&format!(
        "partitions = 2\nartifacts = [{}, {{ name = 'logs', paths = ['logs/'], when = 'failure', retention_days = 1 }}]\n",
        entry("when = 'always'")
    ));
    let workflow = rendered_with(&config, HELPER)?;
    for index in 1..=2 {
        let job = job(&workflow, &format!("rust-test-database-{index}"))?;
        let found = uploads(job);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(
            found[0]["if"],
            "${{ (steps.artifact_stage_0.outcome == 'success') && (always()) }}"
        );
        assert_eq!(
            found[0]["with"]["name"],
            format!("database-failures-{index}")
        );
        assert_eq!(
            found[1]["if"],
            "${{ (steps.artifact_stage_1.outcome == 'success') && (failure()) }}"
        );
        assert_eq!(found[1]["with"]["name"], format!("logs-{index}"));
        assert_eq!(found[1]["with"]["retention-days"], 1);
    }
    Ok(())
}

fn stage_before(job: &Value, upload: &Value) -> Result<Value, String> {
    let all = steps(job);
    let at = all
        .iter()
        .position(|step| step == upload)
        .ok_or("the upload is not a step of the job")?;
    all.get(at.checked_sub(1).ok_or("no step before the upload")?)
        .cloned()
        .ok_or_else(|| "no step before the upload".to_owned())
}

#[test]
fn a_stage_step_copies_the_files_right_before_every_upload_and_shares_its_condition() -> TestResult
{
    let config = group(&format!(
        "partitions = 2\nartifacts = [{}, {{ name = 'logs', paths = ['logs/', 'app/out/*.log'], when = 'failure' }}]\n",
        entry("when = 'always'")
    ));
    let workflow = rendered_with(&config, HELPER)?;
    for index in 1..=2 {
        let job = job(&workflow, &format!("rust-test-database-{index}"))?;
        let found = uploads(job);
        assert_eq!(found.len(), 2, "{found:?}");
        let expected = [
            ("always()", "artifact_stage_0", 0, "--path junit.xml"),
            (
                "failure()",
                "artifact_stage_1",
                1,
                "--path logs/ --path 'app/out/*.log'",
            ),
        ];
        for (upload, (condition, id, position, paths)) in found.into_iter().zip(expected) {
            let stage = stage_before(job, upload)?;
            assert_eq!(stage["id"], id, "{stage}");
            assert_eq!(stage["if"], format!("${{{{ {condition} }}}}"), "{stage}");
            assert_eq!(
                stage["run"],
                format!(
                    ". \"$VELNOR_EXEC\" artifacts stage --into \"$RUNNER_TEMP/velnor-artifacts/{position}\" {paths}"
                ),
                "{stage}"
            );
            assert_eq!(
                upload["with"]["path"],
                format!("${{{{ runner.temp }}}}/velnor-artifacts/{position}")
            );
            assert_eq!(
                upload["if"],
                format!("${{{{ (steps.{id}.outcome == 'success') && ({condition}) }}}}")
            );
        }
    }
    Ok(())
}

#[test]
fn a_group_without_artifacts_uploads_nothing() -> TestResult {
    let workflow = rendered_with(&group("partitions = 2\n"), HELPER)?;
    for index in 1..=2 {
        let job = job(&workflow, &format!("rust-test-database-{index}"))?;
        assert_eq!(uploads(job).len(), 0);
    }
    Ok(())
}

#[test]
fn a_helper_older_than_the_artifacts_release_refuses_the_key() -> TestResult {
    let config = artifacts(&entry("when = 'failure'"));
    let message = refused(&config, "0.1.107")?;
    assert!(
        message.contains("0.1.107") && message.contains("0.1.110") && message.contains("artifacts"),
        "{message}"
    );
    rendered_with(&config, "0.1.110")?;
    Ok(())
}

#[test]
fn a_wildcard_that_can_match_a_secret_name_is_refused_naming_the_secret() -> TestResult {
    for (path, secret) in [
        ("secr*/**", "secret"),
        ("app/s*/x.xml", "secret"),
        ("app/*ecrets/x.xml", "secret"),
        ("app/tok*/x.xml", "token"),
        ("app/pass*/x.xml", "password"),
        ("cred*/x.xml", "credential"),
        ("app/id_*/x.xml", "id_rsa"),
        ("app/*.p*", ".pypirc"),
    ] {
        let config = artifacts(&format!(
            "{{ name = 'a', paths = ['{path}'], when = 'failure' }}"
        ));
        let message = refused(&config, HELPER)?;
        assert!(
            message.contains("test_groups.artifacts.paths") && message.contains(secret),
            "{path:?}: {message}"
        );
    }
    for path in [
        "target/nextest/*/junit.xml",
        "app/*/report.xml",
        "logs/**/*.log",
        "app/out/*.xml",
        "app/junit-*.xml",
    ] {
        let config = artifacts(&format!(
            "{{ name = 'a', paths = ['{path}'], when = 'failure' }}"
        ));
        rendered_with(&config, HELPER)?;
    }
    Ok(())
}
