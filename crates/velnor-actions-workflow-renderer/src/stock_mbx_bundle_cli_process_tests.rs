use super::*;
use std::os::unix::fs::PermissionsExt;

pub(super) fn exercise_fast_exit_import(
    lifecycle: &MbxBundleCacheSteps,
    fixture: &Fixture,
    writer: &Stage,
) -> Result<(), String> {
    let stage = fixture.stage("fast-exit-import", "810016", false)?;
    copy_bundle(&writer.bundle, &stage.bundle)?;
    check_empty_store(&stage, &primary_selection(&stage))?;
    let writer_before = tree_hash(&writer.bundle)?;
    let marker = stage.root.join("fast-exit-arguments");
    let executable = stage.root.join("bin/mbx");
    fs::remove_file(&executable).map_err(|err| err.to_string())?;
    fs::write(
        &executable,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$VELNOR_FAST_EXIT_MARKER\"\nexec /bin/true\n",
    )
    .map_err(|err| err.to_string())?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
        .map_err(|err| err.to_string())?;
    let imported = run_shell(
        &lifecycle.restore[1],
        &stage,
        &restore_inputs("success", CACHE_KEY),
        &stage.project,
        &stage.temp,
        &[("VELNOR_FAST_EXIT_MARKER", marker.as_os_str())],
        "fixture-fast-exit-import",
    )?;
    check_outputs(&imported, true, true)?;
    let selected = selection(&stage, &imported.outputs)?;
    let actual_args: Vec<_> = fs::read_to_string(&marker)
        .map_err(|err| err.to_string())?
        .lines()
        .map(str::to_owned)
        .collect();
    let expected_args = vec![
        "cache".to_owned(),
        "import".to_owned(),
        stage
            .bundle
            .to_str()
            .ok_or("fixture bundle path is not UTF-8")?
            .to_owned(),
    ];
    if actual_args != expected_args
        || imported.outputs.get("mbx_quiesced").map(String::as_str) != Some("true")
        || selected.store != stage.primary
        || stage.bundle.exists()
        || fs::symlink_metadata(&stage.bundle).is_ok()
        || tree_hash(&writer.bundle)? != writer_before
    {
        return Err(format!(
            "immediate-exit process supervision was unsafe: args={actual_args:?} outputs={:?}",
            imported.outputs
        ));
    }
    check_empty_store(&stage, &selected)?;
    eprintln!(
        "process-supervision-only fixture: immediate-exit executable args={actual_args:?}; actual MBX cache semantics are not claimed"
    );
    Ok(())
}
