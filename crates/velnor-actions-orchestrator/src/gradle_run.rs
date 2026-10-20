//! `gradle-run-v1`: the Velnor-owned Gradle step.
//!
//! One staged-helper operation does everything that must not be repository
//! text: it re-verifies the Gradle wrapper (gates 1-3 of the stack contract)
//! before `./gradlew` first runs, installs the project's exact JDK through
//! Mise, writes the Velnor-owned Gradle user home (toolchain pinning and the
//! Maven-bundle init script), and runs one `--continue` Gradle invocation
//! with a cleared environment. Its whole input is the generated step env.

use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_actions_gradle::script::{INIT_SCRIPT_NAME, init_script, user_home_properties};
use velnor_actions_gradle::version::parse_java;
use velnor_actions_gradle::wrapper::{self, JAR_PATH, PROPERTIES_PATH};

use crate::OrchestratorError;

/// Operation tag of the step.
pub const GRADLE_RUN_OP: &str = velnor_actions_contract::GRADLE_RUN_OPERATION;

/// Env key: Gradle root, repository-relative.
pub(crate) const ROOT_ENV: &str = "VELNOR_GRADLE_ROOT";
/// Env key: space-separated Gradle task paths.
pub(crate) const TASKS_ENV: &str = "VELNOR_GRADLE_TASKS";
/// Env key: `--max-workers`.
pub(crate) const WORKERS_ENV: &str = "VELNOR_GRADLE_WORKERS";
/// Env key: `1` when the job's PostgreSQL service is up.
pub(crate) const DATABASE_ENV: &str = "VELNOR_GRADLE_DATABASE";
/// Env key: the resolved project JDK.
pub(crate) const JAVA_ENV: &str = "VELNOR_TOOL_JAVA";

/// Validated step input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GradleRunSpec {
    pub(crate) root: String,
    pub(crate) tasks: Vec<String>,
    pub(crate) workers: u8,
    pub(crate) database: bool,
    pub(crate) java: String,
}

fn fail(code: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: code.to_owned(),
    }
}

fn plain(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

impl GradleRunSpec {
    /// Parse and validate from a variable lookup.
    pub(crate) fn parse(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, OrchestratorError> {
        let root = get(ROOT_ENV).ok_or_else(|| fail("gradle_root_missing"))?;
        if root != "."
            && (!plain(&root) || root.starts_with('/') || root.split('/').any(|s| s == ".."))
        {
            return Err(fail("gradle_root_invalid"));
        }
        let tasks: Vec<String> = get(TASKS_ENV)
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let task_ok = |task: &String| {
            task.starts_with(':')
                && task[1..]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':'))
        };
        if tasks.is_empty() || !tasks.iter().all(task_ok) {
            return Err(fail("gradle_tasks_invalid"));
        }
        let workers = get(WORKERS_ENV)
            .and_then(|text| text.parse::<u8>().ok())
            .filter(|n| (1..=64).contains(n))
            .ok_or_else(|| fail("gradle_workers_invalid"))?;
        let java = get(JAVA_ENV).ok_or_else(|| fail("gradle_java_missing"))?;
        parse_java(&java).map_err(|err| fail(&err.to_string()))?;
        Ok(Self {
            root,
            tasks,
            workers,
            database: get(DATABASE_ENV).as_deref() == Some("1"),
            java,
        })
    }
}

/// Everything the run needs from the machine, injectable for tests.
#[derive(Debug, Clone)]
pub(crate) struct Machine {
    pub(crate) workspace: PathBuf,
    pub(crate) runner_temp: PathBuf,
    pub(crate) mise: PathBuf,
    pub(crate) path: String,
}

fn io_err(what: &str, err: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(what.to_owned(), err.to_string())
}

fn mise_output(machine: &Machine, args: &[&str]) -> Result<String, OrchestratorError> {
    let out = Command::new(&machine.mise)
        .args(["--no-config", "--no-env", "--no-hooks"])
        .args(args)
        .env_clear()
        .env("PATH", &machine.path)
        .env("HOME", machine.runner_temp.join("velnor/home"))
        .output()
        .map_err(|err| io_err("mise", &err))?;
    if !out.status.success() {
        return Err(fail(&format!("mise_failed:{}", args.join(" "))));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Run the step.
pub(crate) fn run(spec: &GradleRunSpec, machine: &Machine) -> Result<(), OrchestratorError> {
    let root = machine.workspace.join(&spec.root);
    let properties = std::fs::read_to_string(root.join(PROPERTIES_PATH)).ok();
    let jar = std::fs::read(root.join(JAR_PATH)).unwrap_or_default();
    wrapper::verify(properties.as_deref(), &jar).map_err(|finding| fail(finding.code()))?;
    let home = machine.runner_temp.join("velnor/home");
    std::fs::create_dir_all(&home).map_err(|err| io_err("home", &err))?;
    let target = format!("java@{}", spec.java);
    mise_output(machine, &["install", &target])?;
    let java_home = mise_output(machine, &["where", &target])?;
    if !Path::new(&java_home).join("bin/java").exists() {
        return Err(fail("jdk_missing_bin_java"));
    }
    let gradle_home = machine.runner_temp.join("velnor/gradle");
    let init_dir = gradle_home.join("init.d");
    std::fs::create_dir_all(&init_dir).map_err(|err| io_err("gradle home", &err))?;
    std::fs::write(
        gradle_home.join("gradle.properties"),
        user_home_properties(),
    )
    .map_err(|err| io_err("gradle.properties", &err))?;
    std::fs::write(init_dir.join(INIT_SCRIPT_NAME), init_script())
        .map_err(|err| io_err("init script", &err))?;
    let mut command = Command::new(root.join("gradlew"));
    command
        .current_dir(&root)
        .env_clear()
        .env("PATH", format!("{java_home}/bin:{}", machine.path))
        .env("HOME", &home)
        .env("JAVA_HOME", &java_home)
        .env("GRADLE_USER_HOME", &gradle_home)
        .env(
            "VELNOR_MAVEN_ROOT",
            machine.runner_temp.join("velnor/deps/maven"),
        )
        .args([
            "--console=plain",
            "--no-daemon",
            "--continue",
            "--dependency-verification",
            "strict",
        ])
        .arg(format!("--max-workers={}", spec.workers))
        .args(&spec.tasks);
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
    ] {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
    if spec.database {
        command
            .env("POSTGRESQL_DB_HOST", "127.0.0.1")
            .env("POSTGRESQL_DB_PORT", "40000");
    }
    let status = command.status().map_err(|err| io_err("gradlew", &err))?;
    if status.success() {
        Ok(())
    } else {
        Err(fail(&format!("gradle_failed:{status}")))
    }
}

/// Run `gradle-run-v1` from the step environment.
///
/// # Errors
/// Any validation, wrapper-gate, toolchain or Gradle failure.
pub fn run_gradle_run() -> Result<(), OrchestratorError> {
    let get = |key: &str| std::env::var(key).ok();
    let spec = GradleRunSpec::parse(&get)?;
    let machine = Machine {
        workspace: std::env::current_dir().map_err(|err| io_err("cwd", &err))?,
        runner_temp: PathBuf::from(get("RUNNER_TEMP").ok_or_else(|| fail("runner_temp_missing"))?),
        mise: PathBuf::from("mise"),
        path: get("PATH").unwrap_or_default(),
    };
    run(&spec, &machine)
}

#[cfg(test)]
#[path = "gradle_run_tests.rs"]
mod tests;
