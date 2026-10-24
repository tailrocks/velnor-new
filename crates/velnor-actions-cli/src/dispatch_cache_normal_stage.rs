use std::env;
use std::path::Path;
use std::process::ExitCode;

pub(crate) const OP: &str = "normal-tools-cache-stage-path-v1";

const KEY_ENV: &str = "VELNOR_NORMAL_CACHE_KEY";

pub(crate) fn run() -> ExitCode {
    let result = read_stage_path(|name| env::var(name));
    let path = match result {
        Ok(path) => path,
        Err(problem) => return fail(problem),
    };
    let Some(output) = env::var_os(super::GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty())
    else {
        return fail("missing_github_output");
    };
    let body = format!("stage_path={path}\n");
    match super::append_owned_output(&output, body.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&format!("append_github_output:{error}")),
    }
}

fn read_stage_path<E>(mut get: E) -> Result<String, &'static str>
where
    E: FnMut(&str) -> Result<String, env::VarError>,
{
    let runner_temp = super::required_path(&mut get, "RUNNER_TEMP")
        .map_err(|_| "normal_cache_runner_temp_invalid")?;
    let key = super::required(&mut get, KEY_ENV).map_err(|_| "normal_cache_key_missing")?;
    stage_path(&runner_temp, &key)
}

pub(crate) fn stage_path(runner_temp: &Path, key: &str) -> Result<String, &'static str> {
    velnor_actions_orchestrator::normal_tools_cache_stage_path(runner_temp, key)
        .map(|path| path.display().to_string())
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: normal tools cache stage failed: {problem}");
    ExitCode::from(1)
}
