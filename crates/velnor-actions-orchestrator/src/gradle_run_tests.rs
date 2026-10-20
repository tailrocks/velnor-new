//! `gradle-run-v1` with a fake `mise` and a fake `gradlew`.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::*;

const JAR: &[u8] =
    include_bytes!("../../velnor-actions-gradle/tests/fixtures/gradle-wrapper-9.5.1.jar");

fn executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
}

struct Fixture {
    dir: tempfile::TempDir,
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
    let jdk = dir.path().join("jdk");
    fs::create_dir_all(jdk.join("bin")).expect("jdk");
    fs::write(jdk.join("bin/java"), "").expect("java");
    let runner_temp = dir.path().join("tmp");
    fs::create_dir_all(&runner_temp).expect("tmp");
    let record = dir.path().join("record");
    executable(
        &root.join("gradlew"),
        &format!(
            "#!/bin/sh\n{{ echo \"cwd=$PWD\"; echo \"args=$*\"; env | sort; }} > {}\n",
            record.display()
        ),
    );
    let mise = dir.path().join("mise");
    executable(
        &mise,
        &format!(
            "#!/bin/sh\nshift 3\ncase \"$1\" in install) exit 0;; where) echo {};; *) exit 9;; esac\n",
            jdk.display()
        ),
    );
    Fixture {
        machine: Machine {
            workspace,
            runner_temp,
            mise,
            path: "/usr/bin:/bin".to_owned(),
        },
        dir,
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

#[test]
fn runs_one_continue_invocation_with_a_cleared_environment() {
    let fx = fixture(JAR);
    run(&spec(), &fx.machine).expect("run");
    let record = fs::read_to_string(fx.dir.path().join("record")).expect("record");
    assert!(record.contains("args=--console=plain --no-daemon --continue --dependency-verification strict --max-workers=4 :core:assemble :core:check"), "{record}");
    assert!(
        record.contains("cwd=") && record.contains("/ws/backend"),
        "{record}"
    );
    assert!(
        record.contains("JAVA_HOME=") && record.contains("POSTGRESQL_DB_PORT=40000"),
        "{record}"
    );
    assert!(
        record.contains("GRADLE_USER_HOME=") && record.contains("VELNOR_MAVEN_ROOT="),
        "{record}"
    );
    let keys: Vec<&str> = record
        .lines()
        .filter_map(|l| l.split_once('=').map(|(k, _)| k))
        .filter(|k| !matches!(*k, "cwd" | "args" | "PWD" | "SHLVL" | "_" | "OLDPWD"))
        .collect();
    for key in keys {
        assert!(
            [
                "PATH",
                "HOME",
                "JAVA_HOME",
                "GRADLE_USER_HOME",
                "VELNOR_MAVEN_ROOT",
                "POSTGRESQL_DB_HOST",
                "POSTGRESQL_DB_PORT",
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "NO_PROXY",
                "http_proxy",
                "https_proxy",
                "no_proxy"
            ]
            .contains(&key),
            "unexpected env {key}"
        );
    }
    let home = fx.machine.runner_temp.join("velnor/gradle");
    assert!(
        fs::read_to_string(home.join("gradle.properties"))
            .expect("props")
            .contains("auto-download=false")
    );
    assert!(home.join("init.d").join(INIT_SCRIPT_NAME).is_file());
}

#[test]
fn a_tampered_wrapper_stops_before_gradle_runs() {
    let mut tampered = JAR.to_vec();
    tampered[200] ^= 1;
    let fx = fixture(&tampered);
    let err = run(&spec(), &fx.machine).expect_err("gate");
    assert!(
        err.to_string().contains("gradle_wrapper_jar_unverified"),
        "{err}"
    );
    assert!(
        !fx.dir.path().join("record").exists(),
        "gradlew must not run"
    );
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
