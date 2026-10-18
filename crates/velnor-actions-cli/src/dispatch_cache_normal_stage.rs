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

fn stage_path(runner_temp: &Path, key: &str) -> Result<String, &'static str> {
    let runner_temp = runner_temp
        .to_str()
        .ok_or("normal_cache_stage_path_invalid")?;
    let Some(identity) = key.strip_prefix("mise-tools-v2-payload-v3-") else {
        return Err("normal_cache_key_invalid");
    };
    if identity.len() != 64
        || !identity
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("normal_cache_key_invalid");
    }
    let key_digest = blake3::hash(key.as_bytes()).to_hex();
    let path = format!("{runner_temp}/velnor/normal-cache-v3/mise_tools/{key_digest}/payload");
    if path.len() > 4_096 {
        return Err("normal_cache_stage_path_invalid");
    }
    Ok(path)
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: normal tools cache stage failed: {problem}");
    ExitCode::from(1)
}

#[cfg(test)]
#[path = "../tests/dispatch_cache_normal_stage_tests.rs"]
mod tests;
