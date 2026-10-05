use super::*;

struct Invocation {
    command: Command,
    label: String,
    output: PathBuf,
    resolved_env: BTreeMap<String, String>,
}

pub(in crate::stock_mbx_bundle_cli_tests) fn run_shell(
    step: &Step,
    stage: &Stage,
    outputs: &BTreeMap<String, String>,
    cwd: &Path,
    temp: &Path,
    extra: &[(&str, &OsStr)],
    label: &str,
) -> Result<StepResult, String> {
    let mut invocation = invocation(step, stage, outputs, cwd, temp, extra, label)?;
    let result = invocation.command.output().map_err(|err| err.to_string())?;
    finish(step, stage, &invocation, &result)
}

fn invocation(
    step: &Step,
    stage: &Stage,
    outputs: &BTreeMap<String, String>,
    cwd: &Path,
    temp: &Path,
    extra: &[(&str, &OsStr)],
    label: &str,
) -> Result<Invocation, String> {
    let run = generated_run(step)?;
    let program = run
        .first()
        .ok_or_else(|| format!("{} has no argv", step.name))?;
    let label = capture_label(&format!("{}-{label}", stage.group));
    capture_shell_step(step, &label)?;
    let output = stage.root.join(format!("{label}.output"));
    fs::write(&output, "").map_err(|err| err.to_string())?;
    let mut command = stage.command(program, &output, temp);
    command
        .args(&run[1..])
        .current_dir(cwd)
        .envs(extra.iter().copied());
    let mut resolved_env = BTreeMap::new();
    for (key, value) in generated_env(step)? {
        let resolved = resolve(value, outputs, stage)?;
        command.env(key, &resolved);
        resolved_env.insert(key.clone(), resolved);
    }
    Ok(Invocation {
        command,
        label,
        output,
        resolved_env,
    })
}

fn finish(
    step: &Step,
    stage: &Stage,
    invocation: &Invocation,
    result: &Output,
) -> Result<StepResult, String> {
    let text = text(result);
    let output_values = pairs(&invocation.output)?;
    capture_runtime(
        &invocation.label,
        &invocation.resolved_env,
        &output_values,
        &env_pairs(&stage.env_file)?,
        result.status.code().unwrap_or(-1),
        &text,
    )?;
    eprintln!(
        "generated shell {} argv={:?} status={} outputs={output_values:?}",
        step.name,
        generated_run(step)?,
        result.status
    );
    Ok(StepResult {
        success: result.status.success(),
        text,
        outputs: output_values,
    })
}

pub(in crate::stock_mbx_bundle_cli_tests) fn import_miss(
    lifecycle: &MbxBundleCacheSteps,
    stage: &Stage,
) -> Result<(StepResult, Selection), String> {
    let result = run_shell(
        &lifecycle.restore[1],
        stage,
        &restore_inputs("success", ""),
        &stage.project,
        &stage.temp,
        &[],
        "miss",
    )?;
    check_outputs(&result, true, true)?;
    let selected = selection(stage, &result.outputs)?;
    Ok((result, selected))
}

pub(in crate::stock_mbx_bundle_cli_tests) fn restore_inputs(
    outcome: &str,
    matched_key: &str,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("restore_outcome".to_owned(), outcome.to_owned()),
        ("restore_matched_key".to_owned(), matched_key.to_owned()),
    ])
}

fn resolve(
    value: &str,
    outputs: &BTreeMap<String, String>,
    stage: &Stage,
) -> Result<String, String> {
    if value == "${{ steps.mbx_cache_restore.outcome }}" {
        return Ok(outputs
            .get("restore_outcome")
            .cloned()
            .unwrap_or_else(|| "success".to_owned()));
    }
    if value == "${{ steps.mbx_cache_restore.outputs.cache-matched-key }}" {
        return Ok(outputs
            .get("restore_matched_key")
            .cloned()
            .unwrap_or_default());
    }
    if value == "${{ steps.mbx_cache_import.outcome }}" {
        return Ok(outputs
            .get("import_outcome")
            .cloned()
            .unwrap_or_else(|| "success".to_owned()));
    }
    if value == "${{ steps.mbx_cache_export.outcome }}" {
        return Ok(outputs
            .get("export_outcome")
            .cloned()
            .unwrap_or_else(|| "success".to_owned()));
    }
    if let Some(key) = value
        .strip_prefix("${{ steps.mbx_cache_preflight.outputs.")
        .and_then(|rest| rest.strip_suffix(" }}"))
    {
        return stage
            .preflight_outputs
            .get(key)
            .cloned()
            .ok_or_else(|| format!("generated preflight omitted {key}"));
    }
    if let Some(key) = value
        .strip_prefix("${{ steps.mbx_cache_import.outputs.")
        .and_then(|rest| rest.strip_suffix(" }}"))
    {
        return Ok(outputs.get(key).cloned().unwrap_or_default());
    }
    if let Some(key) = value
        .strip_prefix("${{ steps.mbx_cache_export.outputs.")
        .and_then(|rest| rest.strip_suffix(" }}"))
    {
        return Ok(outputs.get(key).cloned().unwrap_or_default());
    }
    if value.contains("${{") {
        return Err(format!("unsupported generated env: {value}"));
    }
    Ok(value.to_owned())
}

fn pairs(path: &Path) -> Result<BTreeMap<String, String>, String> {
    fs::read_to_string(path)
        .map_err(|err| err.to_string())?
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .try_fold(BTreeMap::new(), |mut map, (key, value)| {
            if map.insert(key.clone(), value).is_some() {
                Err(format!("duplicate output {key}"))
            } else {
                Ok(map)
            }
        })
}

pub(in crate::stock_mbx_bundle_cli_tests) fn env_pairs(
    path: &Path,
) -> Result<BTreeMap<String, String>, String> {
    let mut pairs = BTreeMap::new();
    for line in fs::read_to_string(path)
        .map_err(|err| err.to_string())?
        .lines()
    {
        if let Some((key, value)) = line.split_once('=') {
            pairs.insert(key.to_owned(), value.to_owned());
        }
    }
    Ok(pairs)
}

pub(in crate::stock_mbx_bundle_cli_tests) fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[path = "stock_mbx_bundle_cli_gated_execution.rs"]
mod gated_execution;
pub(in crate::stock_mbx_bundle_cli_tests) use gated_execution::{
    GateAction, GatedShell, run_shell_gated,
};
