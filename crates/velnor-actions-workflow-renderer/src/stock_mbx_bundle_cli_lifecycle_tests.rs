use super::*;

struct ColdFallbackCase<'a> {
    damaged: &'a Stage,
    target: &'a Path,
    target_hash: &'a str,
    writer_hash: &'a str,
    damaged_hash: &'a str,
    outside_link: Option<(&'a Path, &'a Path)>,
    expected_diagnostic: &'a [&'a str],
}

struct ProducedWriter {
    selection: Selection,
    target: PathBuf,
    target_hash: String,
    bundle_hash: String,
    export_outputs: BTreeMap<String, String>,
}

#[test]
#[ignore = "selected by mandatory official MBX 1.21.1 generated-helper qualification"]
fn generated_mbx_steps_cover_miss_hit_and_separate_corruption_fallbacks() -> Result<(), String> {
    let fixture = Fixture::new()?;
    let lifecycle = steps()?;
    if bundle_path(&lifecycle.restore[0])? != MBX_BUNDLE_CACHE_PATH
        || bundle_path(&lifecycle.save[1])? != MBX_BUNDLE_CACHE_PATH
        || MBX_BUNDLE_CACHE_PATH != ".velnor-mbx-cache/mbx-single-bundle"
    {
        return Err("restore/save do not share the generated bundle path".to_owned());
    }
    let writer = fixture.stage("writer", "810001", false)?;
    let reader = fixture.stage("reader", "810002", false)?;
    let corrupt = fixture.stage("corrupt", "810003", false)?;
    let linked = fixture.stage("linked", "810004", false)?;
    check_cache_path_contract(&lifecycle, &writer, &reader)?;
    let produced = produce(&lifecycle, &writer)?;
    exercise_fast_exit_import(&lifecycle, &fixture, &writer)?;
    reuse(&lifecycle, &writer, &reader, &produced.selection)?;
    exercise_truncated_fallback(
        &lifecycle,
        &writer,
        &corrupt,
        &produced.target,
        &produced.target_hash,
        &produced.bundle_hash,
    )?;
    exercise_symlink_fallback(
        &lifecycle,
        &writer,
        &linked,
        &produced.target,
        &produced.target_hash,
        &produced.bundle_hash,
    )?;
    check_export_gates(&lifecycle)?;
    cleanup_after_export(&lifecycle, &writer, &produced.export_outputs)
}

#[test]
#[ignore = "selected by mandatory official MBX 1.21.1 generated-helper qualification"]
fn generated_restore_failure_runs_producer_cold_and_disables_save() -> Result<(), String> {
    let fixture = Fixture::new()?;
    let lifecycle = steps()?;
    let stage = fixture.stage("restore-failure", "810012", false)?;
    fs::write(
        stage.primary.join("partial-store-state"),
        b"discard primary",
    )
    .map_err(|err| err.to_string())?;
    fs::write(stage.bundle.join("partial-bundle-state"), b"discard bundle")
        .map_err(|err| err.to_string())?;
    let producer_target = stage.root.join("producer-target");
    fs::create_dir(&producer_target).map_err(|err| err.to_string())?;
    fs::write(producer_target.join("sentinel"), b"producer target")
        .map_err(|err| err.to_string())?;
    let target_before = tree_hash(&producer_target)?;
    let result = run_shell(
        &lifecycle.restore[1],
        &stage,
        &restore_inputs("failure", ""),
        &stage.project,
        &stage.temp,
        &[("CARGO_TARGET_DIR", producer_target.as_os_str())],
        "restore-failure",
    )?;
    check_outputs(&result, true, false)?;
    let selected = selection(&stage, &result.outputs)?;
    if lifecycle.restore[0].continue_on_error != Some(true)
        || selected.store != stage.cold
        || stage.bundle.exists()
        || stage.primary.join("partial-store-state").exists()
        || env_pairs(&stage.env_file)?.get("MBX_CACHE_DIR")
            != Some(&stage.cold.display().to_string())
        || tree_hash(&producer_target)? != target_before
    {
        return Err(format!(
            "restore failure did not preserve the producer target and select a cold store: {} {:?}",
            result.text, result.outputs
        ));
    }
    build(&stage, &stage.project, &selected)?;
    if number(&stats(&stage, &selected)?, "cached_compilations")? != 0 {
        return Err("restore-failure cold build reused cached compilations".to_owned());
    }
    check_export_gates(&lifecycle)
}

fn produce(lifecycle: &MbxBundleCacheSteps, stage: &Stage) -> Result<ProducedWriter, String> {
    check_empty_store(stage, &primary_selection(stage))?;
    step_capture::capture_project_sources(stage)?;
    let (miss, selected) = import_miss(lifecycle, stage)?;
    if selected.store != stage.primary {
        return Err("miss did not select its private store".to_owned());
    }
    build(stage, &stage.nested, &selected)?;
    let target = target_dir(&stage.project, &stage.primary)?;
    fs::write(target.join("target-sentinel"), b"preserve-producer-target")
        .map_err(|err| err.to_string())?;
    let target_hash = tree_hash(&target)?;
    let export = run_shell(
        &lifecycle.save[0],
        stage,
        &miss.outputs,
        &stage.nested,
        &stage.temp,
        &[],
        "export",
    )?;
    if !export.success
        || export.outputs.get("ready").map(String::as_str) != Some("true")
        || export.outputs.get("export_available").map(String::as_str) != Some("true")
        || largest_file(&stage.bundle)?.1 == 0
    {
        return Err(format!(
            "miss did not export a nonempty directory: {}",
            export.text
        ));
    }
    check_private_bundle_root(&stage.bundle)?;
    Ok(ProducedWriter {
        selection: selected,
        target,
        target_hash,
        bundle_hash: tree_hash(&stage.bundle)?,
        export_outputs: export.outputs,
    })
}

fn reuse(
    lifecycle: &MbxBundleCacheSteps,
    writer: &Stage,
    reader: &Stage,
    writer_selection: &Selection,
) -> Result<(), String> {
    if writer.project == reader.project || writer.bundle == reader.bundle {
        return Err("writer and reader did not use distinct workspaces".to_owned());
    }
    copy_bundle(&writer.bundle, &reader.bundle)?;
    check_fresh_reader_state(writer, reader)?;
    check_empty_store(reader, &primary_selection(reader))?;
    let hit = run_shell(
        &lifecycle.restore[1],
        reader,
        &restore_inputs("success", CACHE_KEY),
        &reader.nested,
        &reader.temp,
        &[],
        "hit",
    )?;
    check_outputs(&hit, true, true)?;
    if reader.bundle.exists() {
        return Err("successful directory import did not consume bundle".to_owned());
    }
    let selected = selection(reader, &hit.outputs)?;
    if selected.store != reader.primary || selected.group == writer_selection.group {
        return Err("reader reused the producer's per-job export group".to_owned());
    }
    check_fresh_reader_state(writer, reader)?;
    check_imported_cache(reader, &selected)?;
    build(reader, &reader.project, &selected)?;
    if number(&stats(reader, &selected)?, "cached_compilations")? == 0 {
        return Err("fresh reader did not reuse imported compilations".to_owned());
    }
    check_distinct_targets(writer, reader)?;
    Ok(())
}

fn exercise_truncated_fallback(
    lifecycle: &MbxBundleCacheSteps,
    writer: &Stage,
    corrupt: &Stage,
    target: &Path,
    target_hash: &str,
    writer_hash: &str,
) -> Result<(), String> {
    let damaged_hash = truncate_bundle(writer, corrupt)?;
    exercise_cold_fallback(
        lifecycle,
        writer,
        &ColdFallbackCase {
            damaged: corrupt,
            target,
            target_hash,
            writer_hash,
            damaged_hash: &damaged_hash,
            outside_link: None,
            expected_diagnostic: &["digest", "hash", "checksum", "truncated", "unexpected end"],
        },
    )
}

fn exercise_symlink_fallback(
    lifecycle: &MbxBundleCacheSteps,
    writer: &Stage,
    linked: &Stage,
    target: &Path,
    target_hash: &str,
    writer_hash: &str,
) -> Result<(), String> {
    let (linked_hash, outside, link) = add_bundle_symlink(writer, linked)?;
    exercise_cold_fallback(
        lifecycle,
        writer,
        &ColdFallbackCase {
            damaged: linked,
            target,
            target_hash,
            writer_hash,
            damaged_hash: &linked_hash,
            outside_link: Some((&outside, &link)),
            expected_diagnostic: &["non-file entry"],
        },
    )
}

fn exercise_cold_fallback(
    lifecycle: &MbxBundleCacheSteps,
    writer: &Stage,
    case: &ColdFallbackCase<'_>,
) -> Result<(), String> {
    let before = tree_hash(case.target)?;
    let (selected, result) = run_cold_fallback(lifecycle, writer, case.damaged, case.target)?;
    let diagnostic = result
        .text
        .lines()
        .find(|line| line.contains("mbx[error]:"))
        .ok_or_else(|| format!("MBX CLI returned no import diagnostic: {}", result.text))?
        .to_ascii_lowercase();
    if !case
        .expected_diagnostic
        .iter()
        .any(|expected| diagnostic.contains(expected))
    {
        return Err(format!("unexpected MBX import diagnostic: {diagnostic}"));
    }
    check_isolation(case.damaged, case.target, &before, case.target_hash)?;
    check_bundle_unchanged(writer, case.damaged, case.damaged_hash, case.writer_hash)?;
    if let Some((outside, link)) = case.outside_link {
        check_external_link_unchanged(outside, link)?;
    }
    if number(&stats(case.damaged, &selected)?, "objects")? != 0 {
        return Err("cold store contains partial import objects".to_owned());
    }
    build(case.damaged, &case.damaged.nested, &selected)?;
    if number(&stats(case.damaged, &selected)?, "cached_compilations")? != 0 {
        return Err("cold build reused unsafe import state".to_owned());
    }
    check_bundle_unchanged(writer, case.damaged, case.damaged_hash, case.writer_hash)?;
    if let Some((outside, link)) = case.outside_link {
        check_external_link_unchanged(outside, link)?;
    }
    Ok(())
}

fn truncate_bundle(writer: &Stage, corrupt: &Stage) -> Result<String, String> {
    copy_bundle(&writer.bundle, &corrupt.bundle)?;
    let (member, len) = largest_file(&corrupt.bundle)?;
    if len < 2 {
        return Err("bundle member too short to truncate".to_owned());
    }
    OpenOptions::new()
        .write(true)
        .open(&member)
        .and_then(|file| file.set_len(1))
        .map_err(|err| err.to_string())?;
    tree_hash(&corrupt.bundle)
}

fn add_bundle_symlink(
    writer: &Stage,
    linked: &Stage,
) -> Result<(String, PathBuf, PathBuf), String> {
    copy_bundle(&writer.bundle, &linked.bundle)?;
    let outside = writer.root.join("external-sentinel");
    fs::write(&outside, b"outside-bundle-sentinel").map_err(|err| err.to_string())?;
    let (member, _) = largest_file(&linked.bundle)?;
    let link = member
        .parent()
        .ok_or("bundle member has no parent")?
        .join("external-link");
    symlink(&outside, &link).map_err(|err| err.to_string())?;
    Ok((tree_hash(&linked.bundle)?, outside, link))
}

fn run_cold_fallback(
    lifecycle: &MbxBundleCacheSteps,
    writer: &Stage,
    corrupt: &Stage,
    target: &Path,
) -> Result<(Selection, StepResult), String> {
    fs::write(
        corrupt.primary.join("partial-import-state"),
        b"discard partial import",
    )
    .map_err(|err| err.to_string())?;
    let result = run_shell(
        &lifecycle.restore[1],
        corrupt,
        &restore_inputs("success", CACHE_KEY),
        &writer.nested,
        &corrupt.temp,
        &[("CARGO_TARGET_DIR", target.as_os_str())],
        "corrupt-import",
    )?;
    check_outputs(&result, true, false)?;
    let selected = selection(corrupt, &result.outputs)?;
    if selected.store != corrupt.cold
        || env_pairs(&corrupt.env_file)?.get("MBX_CACHE_DIR")
            != Some(&selected.store.display().to_string())
        || corrupt.primary.join("partial-import-state").exists()
    {
        return Err("cold-store output did not propagate through GITHUB_ENV".to_owned());
    }
    Ok((selected, result))
}

fn check_isolation(
    corrupt: &Stage,
    target: &Path,
    before: &str,
    original: &str,
) -> Result<(), String> {
    let cwd = fs::canonicalize(&corrupt.import_cwd).map_err(|err| err.to_string())?;
    let mode = fs::symlink_metadata(&cwd)
        .map_err(|err| err.to_string())?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o700
        || cwd.starts_with(&corrupt.project)
        || tree_hash(target)? != before
        || before != original
        || cwd.join("target").exists()
    {
        return Err("failed import changed target or escaped its private isolated cwd".to_owned());
    }
    Ok(())
}

fn check_bundle_unchanged(
    writer: &Stage,
    corrupt: &Stage,
    corrupt_hash: &str,
    writer_hash: &str,
) -> Result<(), String> {
    if corrupt_hash == writer_hash
        || corrupt.bundle.exists()
        || tree_hash(&writer.bundle)? != writer_hash
    {
        return Err("failed import did not remove only its damaged bundle".to_owned());
    }
    Ok(())
}

fn check_external_link_unchanged(outside: &Path, link: &Path) -> Result<(), String> {
    if fs::read(outside).map_err(|err| err.to_string())? != b"outside-bundle-sentinel"
        || link.exists()
        || fs::symlink_metadata(link).is_ok()
    {
        return Err("import followed an external bundle link or changed its sentinel".to_owned());
    }
    Ok(())
}
