use super::*;

use velnor_actions_contract::MbxProcessGroupOperation;

pub(super) fn generated_run(step: &Step) -> Result<Vec<String>, String> {
    match &step.kind {
        StepKind::Shell { run, .. } => {
            if run.first().map(String::as_str) != Some("sh") {
                return Err(format!("{} is not a generated POSIX shell", step.name));
            }
            if run.last().is_some_and(|body| body.contains(" & ")) {
                return Err(format!(
                    "{} uses background work in a plain shell",
                    step.name
                ));
            }
            Ok(run.clone())
        }
        StepKind::MbxProcessGroupShell(payload) => {
            let operation = payload.operation();
            let expected = match operation {
                MbxProcessGroupOperation::Import => "Import MBX single bundle",
                MbxProcessGroupOperation::Export => "Export MBX single bundle",
            };
            if step.name != expected {
                return Err(format!(
                    "typed MBX operation {operation:?} has unexpected step name {}",
                    step.name
                ));
            }
            let run = crate::stock_mbx_bundle::process_group_run(operation);
            if run.first().map(String::as_str) != Some("bash")
                || run.get(1).map(String::as_str) != Some("-c")
                || run.len() != 3
            {
                return Err(format!("{step:?} has invalid generated Bash argv: {run:?}"));
            }
            Ok(run)
        }
        _ => Err(format!("{} is not a generated shell", step.name)),
    }
}

pub(super) fn generated_env(step: &Step) -> Result<&BTreeMap<String, String>, String> {
    match &step.kind {
        StepKind::Shell { env, .. } | StepKind::Action { env, .. } => Ok(env),
        StepKind::MbxProcessGroupShell(payload) => Ok(payload.env()),
        StepKind::Internal { .. } => Err(format!("{} has no process environment", step.name)),
    }
}

pub(super) fn capture_shell_step(step: &Step, label: &str) -> Result<(), String> {
    let Some(directory) = capture_directory()? else {
        return Ok(());
    };
    let run = generated_run(step)?;
    let [program, option, body] = run.as_slice() else {
        return Err(format!("{} must have three shell argv items", step.name));
    };
    let argv = format!(
        "[{}, {}, {}]\n",
        json_string(program),
        json_string(option),
        json_string(body)
    );
    fs::write(directory.join(format!("{label}.body")), body).map_err(|err| err.to_string())?;
    fs::write(directory.join(format!("{label}.argv")), argv).map_err(|err| err.to_string())?;
    write_map(
        &directory.join(format!("{label}.env")),
        generated_env(step)?,
    )?;
    if let StepKind::MbxProcessGroupShell(payload) = &step.kind {
        fs::write(
            directory.join(format!("{label}.operation")),
            format!("{:?}\n", payload.operation()),
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub(super) fn capture_project_sources(stage: &Stage) -> Result<(), String> {
    let Some(directory) = capture_directory()? else {
        return Ok(());
    };
    let destination = directory.join("writer-project-source");
    for relative in [
        "Cargo.toml",
        "Cargo.lock",
        "app/Cargo.toml",
        "app/src/main.rs",
        "probe/Cargo.toml",
        "probe/src/lib.rs",
    ] {
        let source = stage.project.join(relative);
        let target = destination.join(relative);
        let parent = target
            .parent()
            .ok_or_else(|| format!("project source has no parent: {}", target.display()))?;
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        fs::copy(source, target).map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub(super) fn check_shell_syntax(step: &Step) -> Result<(), String> {
    let run = generated_run(step)?;
    let program = run
        .first()
        .ok_or_else(|| format!("{} has no generated shell argv", step.name))?;
    let label = capture_label(&format!(
        "syntax-{}",
        step.name.to_ascii_lowercase().replace(' ', "-")
    ));
    capture_shell_step(step, &label)?;
    let output = Command::new(program)
        .arg("-n")
        .args(&run[1..])
        .env_clear()
        .output()
        .map_err(|err| format!("cannot syntax-check {}: {err}", step.name))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "generated {} script has invalid syntax ({}): {}",
            step.name,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
