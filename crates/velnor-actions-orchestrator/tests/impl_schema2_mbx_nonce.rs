//! Qualification keys bind both restore paths and imports to one run attempt.

use std::collections::BTreeMap;
use std::fs;

use velnor_actions_contract::{Job, JobTimeout, PermissionLevel, Permissions, StepKind};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::{
    RenderError, render_workflow_ir,
    steps::{mbx_objects_step, shell_step},
};

use crate::impl_common::{TestResult, make_repo};

const QUALIFICATION_SCOPE: &str = "qualification-mbx-v1/single-bundle-roundtrip";
const MBX_VERSION: &str = "1.22.0";
const MBX_ACTION_PIN: &str = "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6";
const PRIMARY_OUTPUT: &str = "${{ steps.mbx-bundle-key.outputs.primary }}";
const CACHE_HIT_OUTPUT: &str = "${{ steps.mbx-bundle.outputs.cache-hit }}";
type ValueResult<T> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn qualification_nonce_scopes_both_keys_and_shared_step_ir() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;
    let writer = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-write-hosted")?;
    let reader = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-read-hosted")?;
    let mut ir = prep.workflow.ir.clone();
    for (id, role) in [
        ("mbx-cache-write-hosted", Some(true)),
        ("mbx-cache-read-hosted", Some(false)),
        ("mbx-production-preservation", None),
    ] {
        assert!(ir.jobs.insert(id.to_owned(), mbx_job(role)?).is_none());
    }
    let rendered = render_workflow_ir(
        &ir,
        prep.config.workflow.policy,
        prep.workflow.support.as_ref(),
        &prep.workflow.context,
    )?;
    for (id, generated) in [
        ("mbx-cache-write-hosted", writer),
        ("mbx-cache-read-hosted", reader),
    ] {
        let direct = crate::impl_schema2_routing::job_body(&rendered, id)?;
        for name in ["Prepare MBX bundle key", "Import MBX single bundle"] {
            assert_eq!(
                run_value(direct, name)?,
                run_value(generated, name)?,
                "{id}: {name}"
            );
        }
    }
    assert_qualification_contract(writer, reader)?;
    assert_production_identity(crate::impl_schema2_routing::job_body(
        &rendered,
        "mbx-production-preservation",
    )?)?;
    Ok(())
}

fn assert_qualification_contract(writer: &str, reader: &str) -> TestResult {
    let key = run_value(writer, "Prepare MBX bundle key")?;
    assert_eq!(key, run_value(reader, "Prepare MBX bundle key")?);
    for required in [
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "run-${run_id}-attempt-${run_attempt}-",
    ] {
        assert!(key.contains(required), "missing `{required}` in {key}");
    }
    let restore = step_body(writer, "Restore MBX single bundle");
    assert!(restore.contains("key: ${{ steps.mbx-bundle-key.outputs.primary }}"));
    assert!(
        !restore.contains("restore-keys:"),
        "qualification writer must restore its exact run-and-attempt key: {restore}"
    );
    assert!(step_body(writer, "Prepare MBX bundle key").contains(QUALIFICATION_SCOPE));
    let writer_import = run_value(writer, "Import MBX single bundle")?;
    assert_before(
        &writer_import,
        "qualification writer restore was not cold",
        "mbx cache import",
    );
    assert!(writer_import.contains("CACHE_HIT") && writer_import.contains("MATCHED"));
    let export = step_body(writer, "Export MBX single bundle");
    assert!(export.contains("steps.mbx-bundle.outputs.cache-hit != 'true'"));
    let export_run = run_value(writer, "Export MBX single bundle")?;
    assert!(
        export_run.contains("mbx cache export")
            && export_run.contains("mbx gc --max-size 0 --json")
    );
    let save = step_body(writer, "Save MBX single bundle");
    assert!(save.contains("steps.mbx-export.outputs.ready == 'true'"));

    let reader_import = run_value(reader, "Import MBX single bundle")?;
    assert_before(
        &reader_import,
        "qualification reader restore was not an exact cache hit",
        "mbx cache import",
    );
    assert!(
        reader_import.contains("$MATCHED\" != \"$EXPECTED_KEY"),
        "{reader_import}"
    );
    let import = step_body(reader, "Import MBX single bundle");
    assert!(
        import.contains(&format!("CACHE_HIT: {CACHE_HIT_OUTPUT}")),
        "{import}"
    );
    assert!(
        import.contains(&format!("EXPECTED_KEY: {PRIMARY_OUTPUT}")),
        "{import}"
    );
    assert_order(
        reader,
        &[
            "Import MBX single bundle",
            "Require imported MBX objects",
            "Compile MBX cache probe",
            "Require reused compilation",
        ],
    );
    assert!(!reader.contains("name: Export MBX single bundle"));
    assert!(!reader.contains("name: Save MBX single bundle"));
    Ok(())
}

fn assert_production_identity(job: &str) -> TestResult {
    let key = run_value(job, "Prepare MBX bundle key")?;
    assert!(!key.contains("GITHUB_RUN_ID") && !key.contains("GITHUB_RUN_ATTEMPT"));
    assert!(!run_value(job, "Import MBX single bundle")?.contains("qualification reader"));
    assert!(!step_body(job, "Import MBX single bundle").contains("CACHE_HIT:"));
    Ok(())
}

fn mbx_job(role: Option<bool>) -> Result<Job, RenderError> {
    let mut rust = shell_step(
        "Pin qualification toolchain",
        vec!["true".to_owned()],
        BTreeMap::from([
            ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
            (
                "MISE_RUSTUP_HOME".to_owned(),
                "${{ runner.temp }}/rustup".to_owned(),
            ),
            (
                "MISE_CARGO_HOME".to_owned(),
                "${{ runner.temp }}/cargo".to_owned(),
            ),
        ]),
    )?;
    rust.condition = None;
    let mut mbx = mbx_objects_step(MBX_ACTION_PIN, false, MBX_VERSION)?;
    if let Some(writer) = role {
        let StepKind::Action { with, .. } = &mut mbx.kind else {
            return Err(RenderError::InvalidWorkflow(
                "mbx_setup_not_action".to_owned(),
            ));
        };
        with.insert(
            "velnor-cache-scope".to_owned(),
            QUALIFICATION_SCOPE.to_owned(),
        );
        with.insert("velnor-cache-writer".to_owned(), writer.to_string());
    }
    Ok(Job {
        display_name: "MBX nonce proof".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::new(45)
            .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?,
        needs: if role == Some(false) {
            vec!["mbx-cache-write-hosted".to_owned()]
        } else {
            Vec::new()
        },
        condition: None,
        permissions: Some(Permissions {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: if role == Some(true) {
                PermissionLevel::Write
            } else {
                PermissionLevel::Read
            },
        }),
        environment: None,
        steps: vec![rust, mbx],
    })
}

fn run_value(job: &str, name: &str) -> ValueResult<String> {
    let scalar = step_body(job, name)
        .lines()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .ok_or_else(|| format!("missing run value for {name}"))?;
    Ok(serde_json::from_str(scalar)?)
}

fn step_body<'a>(job: &'a str, name: &str) -> &'a str {
    let needle = format!("- name: {name}");
    let Some(start) = job.find(&needle) else {
        assert!(job.contains(&needle), "missing `{needle}` in {job}");
        return job;
    };
    let tail = &job[start..];
    let end = tail[needle.len()..]
        .find("\n      - name:")
        .map_or(tail.len(), |offset| needle.len() + offset);
    &tail[..end]
}

fn assert_before(body: &str, first: &str, second: &str) {
    let a = body.find(first);
    let b = body.find(second);
    assert!(
        a.is_some() && b.is_some_and(|b| a.is_some_and(|a| a < b)),
        "{body}"
    );
}

fn assert_order(job: &str, names: &[&str]) {
    let mut offset = 0;
    for name in names {
        let at = job[offset..].find(name);
        assert!(at.is_some(), "missing or out of order `{name}` in {job}");
        offset += at.unwrap_or_default() + name.len();
    }
}

#[cfg(unix)]
#[test]
fn generated_shell_distinguishes_attempts_and_enforces_exact_restore() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;
    let writer = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-write-hosted")?;
    let reader = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-read-hosted")?;
    let key_run = run_value(writer, "Prepare MBX bundle key")?;
    let first = execute_key(&key_run, "731", "1")?;
    let second = execute_key(&key_run, "731", "2")?;
    let third = execute_key(&key_run, "732", "1")?;
    assert_ne!(first.0, second.0);
    assert_ne!(first.1, second.1);
    assert_ne!(first.0, third.0);
    assert_ne!(first.1, third.1);
    assert!(first.0.starts_with(&first.1));

    let writer_import = run_value(writer, "Import MBX single bundle")?;
    let reader_import = run_value(reader, "Import MBX single bundle")?;
    execute_import(&writer_import, "false", "", &first.0, false)?;
    assert!(execute_import(&writer_import, "true", "", &first.0, false).is_err());
    assert!(execute_import(&writer_import, "false", &first.0, &first.0, false).is_err());
    assert!(execute_import(&reader_import, "false", &first.0, &first.0, false).is_err());
    assert!(execute_import(&reader_import, "true", "stale-key", &first.0, false).is_err());
    assert!(execute_import(&reader_import, "true", "", &first.0, false).is_err());
    let log = execute_import(&reader_import, "true", &first.0, &first.0, true)?;
    assert!(log.contains("cache import"), "{log}");
    Ok(())
}

#[cfg(unix)]
fn execute_key(script: &str, run_id: &str, attempt: &str) -> ValueResult<(String, String)> {
    use std::process::Command;

    let temp = tempfile::tempdir()?;
    let runner_temp = temp.path().join("runner-temp");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&runner_temp)?;
    fs::create_dir_all(&bin)?;
    executable(
        &bin.join("mise"),
        "#!/bin/sh\nprintf 'rustc 1.98.1 test\n'\n",
    )?;
    executable(
        &bin.join("sha256sum"),
        &format!("#!/bin/sh\nprintf '{}  %s\\n' \"$1\"\n", "0".repeat(64)),
    )?;
    let output_file = temp.path().join("github-output");
    let path = test_path(&bin)?;
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("PATH", path)
        .env("RUNNER_TEMP", &runner_temp)
        .env("GITHUB_OUTPUT", &output_file)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env(
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/qualification.yml@refs/heads/main",
        )
        .env("GITHUB_SHA", "a".repeat(40))
        .env("GITHUB_RUN_ID", run_id)
        .env("GITHUB_RUN_ATTEMPT", attempt)
        .env("MBX_VERSION", MBX_VERSION)
        .env("MBX_EXPECTED_VERSION", MBX_VERSION)
        .env("MBX_GENERATION", "velnor-mbx-1.22.0")
        .env("MBX_CACHE_SCOPE", QUALIFICATION_SCOPE)
        .env("MBX_MATRIX_CONTEXT", "{}")
        .env("MBX_BASE_SHA", "")
        .env("RUSTUP_TOOLCHAIN", "1.98.1")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = fs::read_to_string(output_file)?;
    let primary = output_value(&result, "primary")?;
    let prefix = output_value(&result, "prefix")?;
    Ok((primary.to_owned(), prefix.to_owned()))
}

#[cfg(unix)]
fn execute_import(
    script: &str,
    cache_hit: &str,
    matched: &str,
    expected: &str,
    bundle: bool,
) -> ValueResult<String> {
    use std::process::Command;

    let temp = tempfile::tempdir()?;
    let runner_temp = temp.path().join("runner-temp");
    let bin = temp.path().join("bin");
    fs::create_dir_all(&runner_temp)?;
    fs::create_dir_all(&bin)?;
    if bundle {
        fs::create_dir_all(runner_temp.join("mbx-single-bundle"))?;
    }
    executable(&bin.join("df"), "#!/bin/sh\nexit 0\n")?;
    let log = temp.path().join("mbx.log");
    executable(
        &bin.join("mbx"),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", log.display()),
    )?;
    let output = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("PATH", test_path(&bin)?)
        .env("RUNNER_TEMP", &runner_temp)
        .env("GITHUB_OUTPUT", temp.path().join("github-output"))
        .env("GITHUB_ENV", temp.path().join("github-env"))
        .env("GITHUB_RUN_ID", "731")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("CACHE_HIT", cache_hit)
        .env("MATCHED", matched)
        .env("EXPECTED_KEY", expected)
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(fs::read_to_string(log).unwrap_or_default())
}

#[cfg(unix)]
fn executable(path: &std::path::Path, body: &str) -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(unix)]
fn test_path(bin: &std::path::Path) -> ValueResult<std::ffi::OsString> {
    let mut paths = vec![bin.to_owned()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    Ok(std::env::join_paths(paths)?)
}

#[cfg(unix)]
fn output_value<'a>(output: &'a str, name: &str) -> ValueResult<&'a str> {
    output
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name}=")))
        .ok_or_else(|| format!("missing {name} output in {output}").into())
}
