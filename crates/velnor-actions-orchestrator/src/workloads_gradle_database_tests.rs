//! Execute the emitted lifecycle against a local Docker protocol fixture.

use super::{PostgresFixture, steps};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use velnor_actions_contract::config::PostgresBinding;
use velnor_actions_contract::{Step, StepKind};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture() -> PostgresFixture {
    PostgresFixture {
        user: "fixture_user".to_owned(),
        password: "fixture_password".to_owned(),
        databases: vec!["fixture_main".to_owned(), "fixture_other".to_owned()],
        bindings: BTreeMap::from([
            ("APP_DB_HOST".to_owned(), PostgresBinding::Host),
            ("APP_DB_PORT".to_owned(), PostgresBinding::Port),
            ("APP_DB_USERNAME".to_owned(), PostgresBinding::User),
            ("APP_DB_PASSWORD".to_owned(), PostgresBinding::Password),
            (
                "APP_DATASOURCE_URL".to_owned(),
                PostgresBinding::JdbcUrl {
                    database: "fixture_other".to_owned(),
                },
            ),
        ]),
    }
}

const DOCKER_FIXTURE: &str = r#"#!/bin/bash
set -euo pipefail
test "$1" = --host && test "$2" = unix:///var/run/docker.sock
test -z "${DOCKER_CONTEXT+x}" && test -z "${DOCKER_HOST+x}"
test -z "${GITHUB_TOKEN+x}" && test -z "${GH_TOKEN+x}"
test -z "${BASH_ENV:-}"
shift 2
printf '%s\n' "$*" >> "$FIXTURE_LOG"
case "$1" in
run) printf '%064d\n' 0 ;;
exec) if test "$3" = psql && test "$FIXTURE_FAILURE" = 1; then exit 17; fi
      if test "$3" = pg_isready && test "$FIXTURE_FAILURE" = 2; then exit 18; fi ;;
inspect) if [[ "$3" = *Config.Image* ]]; then
           if test "$FIXTURE_FAILURE" = 5; then printf 'foreign-image|postgres|77|1|fixture_job\n';
           else printf '%s|postgres|77|1|fixture_job\n' "$FIXTURE_IMAGE"; fi
         else printf '43123\n'; fi ;;
ps) if test "$FIXTURE_FAILURE" != 3 && test ! -e "$FIXTURE_LOG.removed"; then printf '%064d\n' 0; fi ;;
rm) if test "$FIXTURE_FAILURE" = 4; then touch "$FIXTURE_LOG.removed"; exit 19; fi ;;
*) exit 99 ;;
esac
"#;

fn execute(
    step: &Step,
    root: &std::path::Path,
    failure: u8,
) -> Result<bool, Box<dyn std::error::Error>> {
    let StepKind::Shell { run, env } = &step.kind else {
        return Err("expected shell step".into());
    };
    assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin:/bin"));
    let mut resolved = env.clone();
    // The behavioral harness substitutes its protocol fixture for trusted OS binaries.
    resolved.insert(
        "PATH".to_owned(),
        format!("{}:/usr/bin:/bin", root.join("bin").display()),
    );
    for (key, value) in [
        ("VELNOR_FIXTURE_RUN", "77"),
        ("VELNOR_FIXTURE_ATTEMPT", "1"),
        ("VELNOR_FIXTURE_JOB", "fixture_job"),
    ] {
        resolved.insert(key.to_owned(), value.to_owned());
    }
    if resolved.contains_key("VELNOR_FIXTURE_CID") {
        let output = fs::read_to_string(root.join("output"))?;
        let cid = output
            .trim()
            .strip_prefix("cid=")
            .ok_or("missing CID output")?;
        resolved.insert("VELNOR_FIXTURE_CID".to_owned(), cid.to_owned());
    }
    let status = Command::new(&run[0])
        .args(&run[1..])
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", root.join("bin").display()),
        )
        .env("RUNNER_TEMP", root)
        .env("GITHUB_ENV", root.join("env"))
        .env("GITHUB_OUTPUT", root.join("output"))
        .env("FIXTURE_IMAGE", super::IMAGE)
        .env("FIXTURE_LOG", root.join("log"))
        .env("FIXTURE_FAILURE", failure.to_string())
        .env("DOCKER_HOST", "tcp://external.invalid:2375")
        .env("DOCKER_CONTEXT", "external")
        .env("GITHUB_TOKEN", "external-credential")
        .env("GH_TOKEN", "external-credential")
        .env("BASH_ENV", "/invalid-external-shell-control")
        .envs(resolved)
        .status()?;
    Ok(status.success())
}

fn runner() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir(root.path().join("bin"))?;
    let docker = root.path().join("bin/docker");
    fs::write(&docker, DOCKER_FIXTURE)?;
    fs::set_permissions(docker, fs::Permissions::from_mode(0o700))?;
    let sleep = root.path().join("bin/sleep");
    fs::write(
        &sleep,
        "#!/bin/bash\nprintf 'sleep\\n' >> \"$FIXTURE_LOG\"\n",
    )?;
    fs::set_permissions(sleep, fs::Permissions::from_mode(0o700))?;
    Ok(root)
}

#[test]
fn local_endpoint_bindings_and_always_cleanup_execute() -> TestResult {
    let root = runner()?;
    let (prepare, cleanup) = steps(&fixture())?;
    assert_eq!(cleanup.condition.as_deref(), Some("always()"));
    assert!(execute(&prepare, root.path(), 0)?);
    let env = fs::read_to_string(root.path().join("env"))?;
    assert!(env.contains("APP_DATASOURCE_URL=jdbc:postgresql://127.0.0.1:43123/fixture_other\n"));
    assert!(env.contains("APP_DB_HOST=127.0.0.1\n"));
    assert!(env.contains("APP_DB_PASSWORD=fixture_password\n"));
    let log = fs::read_to_string(root.path().join("log"))?;
    assert!(log.contains("--publish 127.0.0.1::5432"));
    assert!(log.contains("--tmpfs /var/lib/postgresql:"));
    assert!(log.contains("@sha256:"));
    assert!(execute(&cleanup, root.path(), 0)?);
    assert!(!root.path().join("velnor-gradle-postgres").exists());
    assert!(fs::read_to_string(root.path().join("log"))?.contains("rm --force --volumes"));
    Ok(())
}

#[test]
fn failed_database_setup_removes_container_and_stays_failed() -> TestResult {
    let root = runner()?;
    let (prepare, cleanup) = steps(&fixture())?;
    assert!(!execute(&prepare, root.path(), 1)?);
    assert!(fs::read_to_string(root.path().join("log"))?.contains("rm --force --volumes"));
    assert!(!root.path().join("env").exists());
    assert!(execute(&cleanup, root.path(), 0)?);
    Ok(())
}

#[test]
fn untrusted_literals_or_reserved_environment_never_emit_shell() {
    for attack in ["fixture'; touch /tmp/attack; '", "$(id)", "bad\nvalue"] {
        let mut data = fixture();
        data.password = attack.to_owned();
        assert!(steps(&data).is_err());
    }
    for key in [
        "GITHUB_DB_HOST",
        "PATH",
        "JAVA_TOOL_OPTIONS",
        "APP_DB_PASSWORD",
    ] {
        let mut data = fixture();
        data.bindings.insert(key.to_owned(), PostgresBinding::Host);
        assert!(steps(&data).is_err());
    }
    let mut data = fixture();
    data.bindings.insert(
        "APP_DATASOURCE_URL".to_owned(),
        PostgresBinding::JdbcUrl {
            database: "undeclared".to_owned(),
        },
    );
    assert!(steps(&data).is_err());
}

#[test]
fn readiness_timeout_is_bounded_and_removes_container() -> TestResult {
    let root = runner()?;
    let (prepare, cleanup) = steps(&fixture())?;
    assert!(!execute(&prepare, root.path(), 2)?);
    let log = fs::read_to_string(root.path().join("log"))?;
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("pg_isready"))
            .count(),
        60
    );
    assert!(log.contains("rm --force --volumes"));
    assert!(!root.path().join("env").exists());
    assert!(execute(&cleanup, root.path(), 0)?);
    Ok(())
}

#[test]
fn already_removed_container_cleanup_still_deletes_state() -> TestResult {
    let root = runner()?;
    let (prepare, cleanup) = steps(&fixture())?;
    assert!(execute(&prepare, root.path(), 0)?);
    assert!(execute(&cleanup, root.path(), 3)?);
    assert!(!root.path().join("velnor-gradle-postgres").exists());
    Ok(())
}

#[test]
fn container_disappearing_during_removal_still_cleans_state() -> TestResult {
    let root = runner()?;
    let (prepare, cleanup) = steps(&fixture())?;
    assert!(execute(&prepare, root.path(), 0)?);
    assert!(execute(&cleanup, root.path(), 4)?);
    assert!(!root.path().join("velnor-gradle-postgres").exists());
    Ok(())
}

#[test]
fn mutable_state_cannot_redirect_controller_owned_cleanup() -> TestResult {
    let root = runner()?;
    let (prepare, cleanup) = steps(&fixture())?;
    assert!(execute(&prepare, root.path(), 0)?);
    let state = root.path().join("velnor-gradle-postgres");
    fs::write(state.join("cid"), "f".repeat(64))?;
    let protected = root.path().join("protected");
    fs::write(&protected, "retained")?;
    std::os::unix::fs::symlink(&protected, state.join("present"))?;
    assert!(execute(&cleanup, root.path(), 0)?);
    assert_eq!(fs::read_to_string(protected)?, "retained");
    let log = fs::read_to_string(root.path().join("log"))?;
    assert!(!log.contains(&"f".repeat(64)));
    Ok(())
}

#[test]
fn foreign_container_ownership_blocks_removal() -> TestResult {
    let root = runner()?;
    let (prepare, cleanup) = steps(&fixture())?;
    assert!(execute(&prepare, root.path(), 0)?);
    assert!(!execute(&cleanup, root.path(), 5)?);
    assert!(!fs::read_to_string(root.path().join("log"))?.contains("rm --force"));
    Ok(())
}

#[test]
fn malformed_controller_output_blocks_docker_call() -> TestResult {
    let root = runner()?;
    let (prepare, cleanup) = steps(&fixture())?;
    assert!(execute(&prepare, root.path(), 0)?);
    let before = fs::read_to_string(root.path().join("log"))?;
    fs::write(root.path().join("output"), "cid=not-a-container\n")?;
    assert!(!execute(&cleanup, root.path(), 0)?);
    assert_eq!(fs::read_to_string(root.path().join("log"))?, before);
    Ok(())
}
