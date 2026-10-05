use super::*;

#[path = "stock_mbx_bundle_cli_execution.rs"]
mod execution;
pub(super) use execution::{
    GateAction, GatedShell, env_pairs, import_miss, restore_inputs, run_shell, run_shell_gated,
    text,
};

pub(super) fn steps() -> Result<MbxBundleCacheSteps, String> {
    let lifecycle = lifecycle_steps(
        TOOLS_RESTORE_USES,
        TOOLS_SAVE_USES,
        CACHE_KEY,
        CACHE_PREFIX,
        SAVE_CONDITION,
    )
    .map_err(|err| err.to_string())?;
    super::check_lifecycle_env(&lifecycle.restore[1], true)?;
    super::check_lifecycle_env(&lifecycle.save[0], false)?;
    super::check_lifecycle_shell(&lifecycle.restore[1], "bash")?;
    super::check_lifecycle_shell(&lifecycle.save[0], "bash")?;
    super::check_lifecycle_shell(&lifecycle.save[2], "sh")?;
    super::check_shell_syntax(&lifecycle.restore[1])?;
    super::check_shell_syntax(&lifecycle.save[0])?;
    super::check_shell_syntax(&lifecycle.save[2])?;
    super::check_cleanup_env(&lifecycle.save[2])?;
    capture_action_step(&lifecycle.restore[0], &capture_label("cache-restore"))?;
    capture_action_step(&lifecycle.save[1], &capture_label("cache-save"))?;
    Ok(lifecycle)
}

pub(super) fn preflight_step(rustup_home: &OsStr, cargo_home: &Path) -> Result<Step, String> {
    let [preflight, _setup] = generated_preflight_steps(rustup_home, cargo_home)?;
    Ok(preflight)
}

pub(super) fn generated_preflight_steps(
    rustup_home: &OsStr,
    cargo_home: &Path,
) -> Result<[Step; 2], String> {
    let env = BTreeMap::from([
        (
            "MISE_RUSTUP_HOME".to_owned(),
            rustup_home.to_string_lossy().into_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            cargo_home.display().to_string(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
    ]);
    let uses = format!("{MBX_ACTION_NAME}@1687e54eb349cadf61fa38b5813a77875489e8e6");
    let steps = mbx_steps_for_driver(&uses, CompileDriver::Mbx, "1.21.1", "1.98.1", env)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "MBX driver did not create preflight".to_owned())?;
    super::check_shell_syntax(&steps[0])?;
    Ok(steps)
}

pub(super) fn check_local_setup_env(step: &Step, cargo_home: &Path) -> Result<(), String> {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err("generated MBX local setup is not an action".to_owned());
    };
    let expected = [
        (
            "MBX_CACHE_DIR",
            "${{ steps.mbx_cache_preflight.outputs.cache_dir }}",
        ),
        (
            "MBX_CACHE_EXPORT_GROUP",
            "${{ steps.mbx_cache_preflight.outputs.export_group }}",
        ),
        (crate::cache_steps::MBX_CACHE_MODE_ENV, "read"),
        (
            crate::cache_steps::MBX_SHARE_OUT_DIR_ENV,
            crate::cache_steps::MBX_SHARE_OUT_DIR_VALUE,
        ),
        ("VELNOR_MBX_VERSION", "1.21.1"),
        ("RUSTUP_TOOLCHAIN", "1.98.1"),
    ];
    for (key, value) in expected {
        if env.get(key).map(String::as_str) != Some(value) {
            return Err(format!("generated local setup has wrong {key}: {env:?}"));
        }
    }
    let cargo_home = cargo_home.display().to_string();
    if env.get("MISE_CARGO_HOME") != Some(&cargo_home)
        || env.get("CARGO_HOME") != Some(&cargo_home)
        || env.get("RUSTUP_HOME") != env.get("MISE_RUSTUP_HOME")
        || with.get("version").map(String::as_str) != Some("1.21.1")
        || with.get("toolchain").map(String::as_str) != Some("1.98.1")
        || with.get("backend").map(String::as_str) != Some("local")
        || with.get("github-cache-mode").map(String::as_str) != Some("objects")
        || !uses.starts_with(&format!("{MBX_ACTION_NAME}@"))
    {
        return Err(format!(
            "generated local setup has wrong toolchain/store contract: {step:?}"
        ));
    }
    Ok(())
}
pub(super) fn selection(
    stage: &Stage,
    values: &BTreeMap<String, String>,
) -> Result<Selection, String> {
    let store = PathBuf::from(values.get("cache_dir").ok_or("missing cache_dir")?);
    let group = values.get("export_group").ok_or("missing export_group")?;
    if (store != stage.primary && store != stage.cold)
        || group != stage.group.as_str()
        || !store.starts_with(&stage.job_root)
    {
        return Err(format!(
            "invalid generated MBX selection: {} {group}",
            store.display()
        ));
    }
    Ok(Selection {
        store,
        group: group.clone(),
    })
}

pub(super) fn primary_selection(stage: &Stage) -> Selection {
    Selection {
        store: stage.primary.clone(),
        group: stage.group.clone(),
    }
}

pub(super) fn check_empty_store(stage: &Stage, selected: &Selection) -> Result<(), String> {
    let output = stats(stage, selected)?;
    let body = std::str::from_utf8(&output.stdout).map_err(|err| err.to_string())?;
    let expected_store = format!("\"store\": \"{}/actions\"", selected.store.display());
    if !body.contains(&expected_store)
        || number(&output, "objects")? != 0
        || number(&output, "cached_compilations")? != 0
        || number(&output, "builds")? != 0
        || number(&output, "shared_store_bytes")? != 0
    {
        return Err(format!(
            "pre-import MBX store was not fresh/private at {}: {body}",
            selected.store.display()
        ));
    }
    Ok(())
}

fn run_mbx(
    stage: &Stage,
    args: &[OsString],
    cwd: &Path,
    selected: &Selection,
) -> Result<Output, String> {
    let output_path = stage.root.join("mbx.output");
    fs::write(&output_path, "").map_err(|err| err.to_string())?;
    let mut command = stage.command(
        stage.binary.to_str().ok_or("non-UTF-8 MBX path")?,
        &output_path,
        &stage.temp,
    );
    let github_env = env_pairs(&stage.env_file)?;
    let store = github_env
        .get("MBX_CACHE_DIR")
        .map_or_else(|| selected.store.clone(), PathBuf::from);
    if store != selected.store {
        return Err(format!(
            "GITHUB_ENV selected {}, helper selected {}",
            store.display(),
            selected.store.display()
        ));
    }
    let group = github_env
        .get("MBX_CACHE_EXPORT_GROUP")
        .map_or(selected.group.as_str(), String::as_str);
    if group != selected.group.as_str() {
        return Err(format!(
            "GITHUB_ENV group {group} differs from selected {}",
            selected.group
        ));
    }
    let result = command
        .env("MBX_CACHE_DIR", store)
        .env("MBX_CACHE_EXPORT_GROUP", group)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|err| err.to_string())?;
    eprintln!("mbx {args:?}: {}", text(&result));
    if result.status.success() {
        Ok(result)
    } else {
        Err(format!("official MBX failed: {}", text(&result)))
    }
}

pub(super) fn build(stage: &Stage, cwd: &Path, selected: &Selection) -> Result<(), String> {
    let mut args = strings(&["build", "--locked", "--offline", "--manifest-path"]);
    args.push(stage.project.join("Cargo.toml").into_os_string());
    run_mbx(stage, &args, cwd, selected)?;
    Ok(())
}

pub(super) fn number(output: &Output, field: &str) -> Result<u64, String> {
    let body = std::str::from_utf8(&output.stdout).map_err(|err| err.to_string())?;
    let key = format!("\"{field}\":");
    body.split_once(&key)
        .map(|(_, rest)| {
            rest.trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .ok_or_else(|| format!("missing MBX stats {field}"))?
        .parse()
        .map_err(|err| format!("MBX stats {field}: {err}"))
}

pub(super) fn stats(stage: &Stage, selected: &Selection) -> Result<Output, String> {
    run_mbx(
        stage,
        &strings(&["stats", "--json"]),
        &stage.project,
        selected,
    )
}

fn strings(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
