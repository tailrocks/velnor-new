use std::ffi::OsString;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::Duration;

use velnor_actions_mise::MiseError;
use velnor_actions_mise::ProcessOutput;
use velnor_actions_mise::command::{IsolatedCommand, OUTPUT_CAPTURE_LIMIT_BYTES};

use velnor_actions_gradle::script::{INIT_SCRIPT_NAME, init_script, user_home_properties};
use velnor_actions_gradle::version::parse_java;
use velnor_actions_gradle::wrapper::{self, JAR_PATH, PROPERTIES_PATH};

use crate::OrchestratorError;
use crate::config::deps_layout::{deps_root, maven_root};

pub const GRADLE_RUN_OP: &str = velnor_actions_contract::GRADLE_RUN_OPERATION;

pub(crate) const ROOT_ENV: &str = "VELNOR_GRADLE_ROOT";
pub(crate) const TASKS_ENV: &str = "VELNOR_GRADLE_TASKS";
pub(crate) const WORKERS_ENV: &str = "VELNOR_GRADLE_WORKERS";
pub(crate) const DATABASE_ENV: &str = "VELNOR_GRADLE_DATABASE";
pub(crate) const JAVA_ENV: &str = "VELNOR_TOOL_JAVA";

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

#[derive(Debug, Clone)]
pub(crate) struct Machine {
    pub(crate) workspace: PathBuf,
    pub(crate) runner_temp: PathBuf,
}

pub(crate) type Exec<'a> = &'a dyn Fn(&IsolatedCommand) -> Result<ProcessOutput, MiseError>;

const RUN_TIMEOUT: Duration = Duration::from_hours(6);

fn io_err(what: &str, err: &std::io::Error) -> OrchestratorError {
    OrchestratorError::io(what.to_owned(), err.to_string())
}

fn mise_err(err: &MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: err.to_string(),
    }
}

fn os(text: impl Into<OsString>) -> OsString {
    text.into()
}

pub(crate) fn run(
    spec: &GradleRunSpec,
    machine: &Machine,
    exec: Exec<'_>,
) -> Result<(), OrchestratorError> {
    let root = machine.workspace.join(&spec.root);
    let properties = std::fs::read_to_string(root.join(PROPERTIES_PATH)).ok();
    let jar = std::fs::read(root.join(JAR_PATH)).unwrap_or_default();
    wrapper::verify(properties.as_deref(), &jar).map_err(|finding| fail(finding.code()))?;
    let target = vec![format!("java@{}", spec.java)];
    let install = IsolatedCommand::mise_install(&target).map_err(|err| mise_err(&err))?;
    let installed = exec(&install).map_err(|err| mise_err(&err))?;
    if !installed.success {
        return Err(fail("jdk_install_failed"));
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
    let mut payload = vec![
        os(root.join("gradlew")),
        os("--console=plain"),
        os("--no-daemon"),
        os("--continue"),
        os("--dependency-verification"),
        os("strict"),
        os(format!("--max-workers={}", spec.workers)),
    ];
    payload.extend(spec.tasks.iter().map(os));
    let mut env = vec![
        (os("GRADLE_USER_HOME"), os(&gradle_home)),
        (
            os("VELNOR_MAVEN_ROOT"),
            os(maven_root(&deps_root(&machine.workspace))),
        ),
    ];
    if spec.database {
        env.push((os("POSTGRESQL_DB_HOST"), os("127.0.0.1")));
        env.push((os("POSTGRESQL_DB_PORT"), os("40000")));
    }
    let command = IsolatedCommand::mise_exec(&target, &payload)
        .map_err(|err| mise_err(&err))?
        .with_cwd(root)
        .with_env(&env)
        .map_err(|err| mise_err(&err))?;
    let output = exec(&command).map_err(|err| mise_err(&err))?;
    std::io::stdout()
        .write_all(&output.stdout)
        .and_then(|()| std::io::stderr().write_all(&output.stderr))
        .map_err(|err| io_err("gradle output", &err))?;
    if output.success {
        Ok(())
    } else {
        Err(fail(&format!("gradle_failed:{:?}", output.code)))
    }
}

pub fn run_gradle_run() -> Result<(), OrchestratorError> {
    let get = |key: &str| std::env::var(key).ok();
    let spec = GradleRunSpec::parse(&get)?;
    let machine = Machine {
        workspace: std::env::current_dir().map_err(|err| io_err("cwd", &err))?,
        runner_temp: PathBuf::from(get("RUNNER_TEMP").ok_or_else(|| fail("runner_temp_missing"))?),
    };
    run(&spec, &machine, &|command| {
        command.run_bounded(OUTPUT_CAPTURE_LIMIT_BYTES, RUN_TIMEOUT)
    })
}
