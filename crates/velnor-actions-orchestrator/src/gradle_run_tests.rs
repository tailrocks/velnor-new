//! `gradle-run-v1` with a fake `mise` and a fake `gradlew`.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;

use velnor_actions_mise::ProcessOutput;

use super::*;

const JAR: &[u8] =
    include_bytes!("../../velnor-actions-gradle/tests/fixtures/gradle-wrapper-9.5.1.jar");

struct Fixture {
    _dir: tempfile::TempDir,
    machine: Machine,
}

fn fixture(jar: &[u8]) -> Fixture {
    let dir = tempfile::tempdir().expect("tmp");
    let workspace = dir.path().join("ws");
    let root = workspace.join("backend");
    fs::create_dir_all(root.join("gradle/wrapper")).expect("dirs");
    fs::write(root.join(JAR_PATH), jar).expect("jar");
    fs::write(
        root.join(PROPERTIES_PATH),
        "distributionSha256Sum=bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f\n\
         distributionUrl=https\\://services.gradle.org/distributions/gradle-9.5.1-bin.zip\n",
    )
    .expect("props");
    let runner_temp = dir.path().join("tmp");
    fs::create_dir_all(&runner_temp).expect("tmp");
    Fixture {
        machine: Machine {
            workspace,
            runner_temp,
        },
        _dir: dir,
    }
}

fn spec() -> GradleRunSpec {
    GradleRunSpec {
        root: "backend".to_owned(),
        tasks: vec![":core:assemble".to_owned(), ":core:check".to_owned()],
        workers: 4,
        database: true,
        java: "oracle-graalvm-25.0.3".to_owned(),
    }
}

fn ok_output(success: bool) -> ProcessOutput {
    ProcessOutput {
        stdout: Vec::new(),
        stderr: Vec::new(),
        code: Some(i32::from(!success)),
        signal: None,
        success,
    }
}

#[test]
fn installs_the_jdk_then_runs_one_continue_invocation_through_isolated_mise() {
    let fx = fixture(JAR);
    let seen = RefCell::new(Vec::new());
    run(&spec(), &fx.machine, &|command| {
        seen.borrow_mut()
            .push((command.argv(), command.cwd().cloned(), command.full_env()));
        Ok(ok_output(true))
    })
    .expect("run");
    let seen = seen.into_inner();
    assert_eq!(seen.len(), 2);
    let text = |argv: &[OsString]| {
        argv.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let install = text(&seen[0].0);
    assert!(
        install.contains("install java@oracle-graalvm-25.0.3"),
        "{install}"
    );
    let exec = text(&seen[1].0);
    assert!(
        exec.contains("exec java@oracle-graalvm-25.0.3 --"),
        "{exec}"
    );
    assert!(exec.contains("gradlew --console=plain --no-daemon --continue --dependency-verification strict --max-workers=4 :core:assemble :core:check"), "{exec}");
    assert!(
        seen[1]
            .1
            .as_ref()
            .is_some_and(|cwd| cwd.ends_with("ws/backend"))
    );
    let env: Vec<String> = seen[1]
        .2
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    for key in [
        "GRADLE_USER_HOME",
        "VELNOR_MAVEN_ROOT",
        "POSTGRESQL_DB_HOST",
        "POSTGRESQL_DB_PORT",
    ] {
        assert!(env.iter().any(|k| k == key), "{key} in {env:?}");
    }
    assert!(!env.iter().any(|k| k.contains("TOKEN")), "{env:?}");
    let home = fx.machine.runner_temp.join("velnor/gradle");
    assert!(
        fs::read_to_string(home.join("gradle.properties"))
            .expect("props")
            .contains("auto-download=false")
    );
    assert!(home.join("init.d").join(INIT_SCRIPT_NAME).is_file());
}

#[test]
fn a_tampered_wrapper_stops_before_any_child_runs() {
    let mut tampered = JAR.to_vec();
    tampered[200] ^= 1;
    let fx = fixture(&tampered);
    let calls = RefCell::new(0);
    let err = run(&spec(), &fx.machine, &|_| {
        *calls.borrow_mut() += 1;
        Ok(ok_output(true))
    })
    .expect_err("gate");
    assert!(
        err.to_string().contains("gradle_wrapper_jar_unverified"),
        "{err}"
    );
    assert_eq!(calls.into_inner(), 0);
}

#[test]
fn a_failing_gradle_run_fails_the_step() {
    let fx = fixture(JAR);
    let n = RefCell::new(0);
    let err = run(&spec(), &fx.machine, &|_| {
        *n.borrow_mut() += 1;
        Ok(ok_output(*n.borrow() == 1))
    })
    .expect_err("gradle failure");
    assert!(err.to_string().contains("gradle_failed"), "{err}");
}

#[test]
fn input_validation_is_closed() {
    let base = |key: &str, value: &str| {
        let mut env = std::collections::BTreeMap::from([
            (ROOT_ENV, "backend"),
            (TASKS_ENV, ":a:check"),
            (WORKERS_ENV, "4"),
            (JAVA_ENV, "oracle-graalvm-25.0.3"),
        ]);
        env.insert(key, value);
        GradleRunSpec::parse(&|k| env.get(k).map(|v| (*v).to_owned()))
    };
    assert!(base(ROOT_ENV, "backend").is_ok());
    for (key, bad) in [
        (ROOT_ENV, "/abs"),
        (ROOT_ENV, "../up"),
        (ROOT_ENV, "a b"),
        (TASKS_ENV, "check"),
        (TASKS_ENV, ":a;rm"),
        (TASKS_ENV, ""),
        (WORKERS_ENV, "0"),
        (WORKERS_ENV, "x"),
        (JAVA_ENV, "21"),
        (JAVA_ENV, "temurin-21.0.8 --evil"),
    ] {
        assert!(base(key, bad).is_err(), "{key}={bad}");
    }
}
