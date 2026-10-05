use super::runner::text;
use super::*;

pub(super) fn bundle_path(step: &Step) -> Result<&str, String> {
    match &step.kind {
        StepKind::Action { with, .. } => with
            .get("path")
            .map(String::as_str)
            .ok_or("cache action has no path".to_owned()),
        _ => Err(format!("{} is not an action", step.name)),
    }
}

pub(super) fn check_cache_path_contract(
    lifecycle: &MbxBundleCacheSteps,
    writer: &Stage,
    reader: &Stage,
) -> Result<(), String> {
    let (restore_uses, restore_with) = action_inputs(&lifecycle.restore[0])?;
    let (save_uses, save_with) = action_inputs(&lifecycle.save[1])?;
    let writer_relative = writer
        .bundle
        .strip_prefix(&writer.project)
        .map_err(|err| err.to_string())?;
    let reader_relative = reader
        .bundle
        .strip_prefix(&reader.project)
        .map_err(|err| err.to_string())?;
    let restore_pin = restore_uses.rsplit_once('@').map(|(_, pin)| pin);
    let save_pin = save_uses.rsplit_once('@').map(|(_, pin)| pin);
    if writer.project == reader.project
        || writer_relative != Path::new(MBX_BUNDLE_CACHE_PATH)
        || reader_relative != Path::new(MBX_BUNDLE_CACHE_PATH)
        || restore_uses != TOOLS_RESTORE_USES
        || save_uses != TOOLS_SAVE_USES
        || restore_with.get("path").map(String::as_str) != Some(MBX_BUNDLE_CACHE_PATH)
        || save_with.get("path") != restore_with.get("path")
        || restore_with.get("key").map(String::as_str) != Some(CACHE_KEY)
        || save_with.get("key") != restore_with.get("key")
        || restore_with.get("restore-keys").map(String::as_str) != Some(CACHE_PREFIX)
        || save_with.contains_key("restore-keys")
        || restore_pin.is_none()
        || restore_pin != save_pin
    {
        return Err(
            "cache actions do not share the stable workspace-relative archive path".to_owned(),
        );
    }
    Ok(())
}

pub(super) fn check_fresh_reader_state(writer: &Stage, reader: &Stage) -> Result<(), String> {
    for directory in ["home", "config", "cache", "cargo", "mise-cache"] {
        let writer_path = writer.root.join(directory);
        let reader_path = reader.root.join(directory);
        if writer_path == reader_path
            || !writer_path.is_dir()
            || !reader_path.is_dir()
            || !writer_path.starts_with(&writer.root)
            || !reader_path.starts_with(&reader.root)
        {
            return Err(format!("reader {directory} state is not job-private"));
        }
    }
    for directory in ["home", "cargo"] {
        if fs::read_dir(reader.root.join(directory))
            .map_err(|err| err.to_string())?
            .next()
            .is_some()
        {
            return Err(format!("reader {directory} was not fresh before import"));
        }
    }
    if fs::symlink_metadata(reader.project.join("target")).is_ok() {
        return Err("reader target existed before import".to_owned());
    }
    Ok(())
}

pub(super) fn check_distinct_targets(writer: &Stage, reader: &Stage) -> Result<(), String> {
    let writer_target = target_dir(&writer.project, &writer.primary)?;
    let reader_target = target_dir(&reader.project, &reader.primary)?;
    if writer_target == reader_target
        || !writer_target.starts_with(&writer.primary)
        || !reader_target.starts_with(&reader.primary)
        || super::fixture::path_identity(&writer_target)?
            == super::fixture::path_identity(&reader_target)?
    {
        return Err(
            "reader build did not create an independent target in its private store".to_owned(),
        );
    }
    Ok(())
}

pub(super) fn check_imported_cache(stage: &Stage, selected: &Selection) -> Result<(), String> {
    let output = super::runner::stats(stage, selected)?;
    let body = std::str::from_utf8(&output.stdout).map_err(|err| err.to_string())?;
    let expected_store = format!("\"store\": \"{}/actions\"", selected.store.display());
    if selected.store != stage.primary
        || !body.contains(&expected_store)
        || super::runner::number(&output, "objects")? == 0
    {
        return Err(format!(
            "directory import did not populate the fresh selected MBX store: {body}"
        ));
    }
    Ok(())
}

pub(super) fn cleanup_after_export(
    lifecycle: &MbxBundleCacheSteps,
    stage: &Stage,
    export_outputs: &BTreeMap<String, String>,
) -> Result<(), String> {
    let target_link = stage.project.join("target");
    let target_before = if fs::symlink_metadata(&target_link).is_ok() {
        let target = target_dir(&stage.project, &stage.primary)?;
        Some((target.clone(), tree_hash(&target)?))
    } else {
        None
    };
    let result = super::runner::run_shell(
        &lifecycle.save[2],
        stage,
        export_outputs,
        &stage.project,
        &stage.temp,
        &[],
        "cleanup",
    )?;
    let parent = stage.project.join(".velnor-mbx-cache");
    let lease = stage.project.join(".velnor-mbx-cache.lock");
    if !result.success
        || !result.outputs.is_empty()
        || fs::symlink_metadata(&stage.bundle).is_ok()
        || fs::symlink_metadata(&parent).is_ok()
        || fs::symlink_metadata(&lease).is_ok()
        || !stage.primary.is_dir()
        || !stage.job_root.is_dir()
    {
        return Err(format!(
            "generated cleanup removed an unowned path or failed: {} {:?}",
            result.text, result.outputs
        ));
    }
    if let Some((target, before)) = target_before
        && tree_hash(&target)? != before
    {
        return Err("workspace cleanup changed the producer target".to_owned());
    }
    Ok(())
}

fn action_inputs(step: &Step) -> Result<(&str, &BTreeMap<String, String>), String> {
    match &step.kind {
        StepKind::Action { uses, with, .. } => Ok((uses, with)),
        _ => Err(format!("{} is not a cache action", step.name)),
    }
}

pub(super) fn tree_hash(root: &Path) -> Result<String, String> {
    let output = Command::new("/bin/bash")
        .args([
            "-euo",
            "pipefail",
            "-c",
            r"{ find . -type f -print0 | sort -z | xargs -0 -r sha256sum --; find . -type l -printf 'L\0%p\0%l\0' | sort -z; find . -type d -printf 'D\0%p\0' | sort -z; } | sha256sum",
        ])
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(format!("tree hash failed: {}", text(&output)));
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or("tree hash produced no digest".to_owned())
}

pub(super) fn largest_file(root: &Path) -> Result<(PathBuf, u64), String> {
    let mut largest = None;
    for item in fs::read_dir(root).map_err(|err| err.to_string())? {
        let item = item.map_err(|err| err.to_string())?;
        if item.file_type().map_err(|err| err.to_string())?.is_dir() {
            let found = largest_file(&item.path())?;
            if largest.as_ref().is_none_or(|(_, old)| found.1 > *old) {
                largest = Some(found);
            }
        } else if item.file_type().map_err(|err| err.to_string())?.is_file() {
            let len = item.metadata().map_err(|err| err.to_string())?.len();
            if largest.as_ref().is_none_or(|(_, old)| len > *old) {
                largest = Some((item.path(), len));
            }
        }
    }
    largest.ok_or_else(|| format!("bundle contains no regular files: {}", root.display()))
}

pub(super) fn copy_bundle(source: &Path, destination: &Path) -> Result<(), String> {
    check_empty_destination(destination)?;
    check_private_bundle_root(source)?;
    let source_workspace = bundle_workspace(source)?;
    let destination_workspace = bundle_workspace(destination)?;
    let archive = destination.with_extension("actions-cache.tar");
    create_bundle_archive(&source_workspace, &archive)?;
    check_archive_paths(&archive)?;
    extract_bundle_archive(&archive, &destination_workspace)?;
    fs::remove_file(&archive).map_err(|err| err.to_string())?;
    check_private_bundle_root(destination)
}

fn check_empty_destination(destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(destination).map_err(|err| err.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "restored bundle destination is not a real directory: {}",
            destination.display()
        ));
    }
    check_private_bundle_root(destination)?;
    if fs::read_dir(destination)
        .map_err(|err| err.to_string())?
        .next()
        .is_some()
    {
        return Err(format!(
            "restored bundle destination is not empty: {}",
            destination.display()
        ));
    }
    Ok(())
}

fn bundle_workspace(bundle: &Path) -> Result<PathBuf, String> {
    let workspace = bundle
        .parent()
        .and_then(Path::parent)
        .ok_or("bundle has no workspace root")?;
    if bundle
        .strip_prefix(workspace)
        .map_err(|err| err.to_string())?
        != Path::new(MBX_BUNDLE_CACHE_PATH)
    {
        return Err("archive path does not match the stable Actions/cache input".to_owned());
    }
    Ok(workspace.to_path_buf())
}

fn create_bundle_archive(workspace: &Path, archive: &Path) -> Result<(), String> {
    let archived = Command::new("/usr/bin/tar")
        .args([OsStr::new("-cf"), archive.as_os_str(), OsStr::new("-C")])
        .arg(workspace)
        .arg(MBX_BUNDLE_CACHE_PATH)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|err| err.to_string())?;
    if !archived.status.success() {
        return Err(format!("archive bundle failed: {}", text(&archived)));
    }
    Ok(())
}

fn check_archive_paths(archive: &Path) -> Result<(), String> {
    let listing = Command::new("/usr/bin/tar")
        .args([OsStr::new("-tf"), archive.as_os_str()])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|err| err.to_string())?;
    let entries = String::from_utf8_lossy(&listing.stdout);
    let prefix = format!("{MBX_BUNDLE_CACHE_PATH}/");
    if !listing.status.success()
        || entries.lines().any(|entry| {
            let entry = entry.trim_end_matches('/');
            entry != MBX_BUNDLE_CACHE_PATH && !entry.starts_with(&prefix)
        })
    {
        return Err(format!(
            "archive did not preserve the workspace-relative cache path: {}",
            text(&listing)
        ));
    }
    Ok(())
}

fn extract_bundle_archive(archive: &Path, destination_workspace: &Path) -> Result<(), String> {
    let restored = Command::new("/usr/bin/tar")
        .args([OsStr::new("-xf"), archive.as_os_str(), OsStr::new("-C")])
        .arg(destination_workspace)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(|err| err.to_string())?;
    if !restored.status.success() {
        return Err(format!("restore bundle failed: {}", text(&restored)));
    }
    Ok(())
}

pub(super) fn check_private_bundle_root(root: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(root).map_err(|err| err.to_string())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(format!(
            "bundle root is not a private real directory: {}",
            root.display()
        ));
    }
    Ok(())
}

pub(super) fn check_outputs(
    result: &StepResult,
    store_ready: bool,
    export_allowed: bool,
) -> Result<(), String> {
    if !result.success {
        return Err(format!("generated helper failed: {}", result.text));
    }
    for (key, expected) in [
        ("store_ready", store_ready),
        ("export_allowed", export_allowed),
    ] {
        if result.outputs.get(key).map(String::as_str)
            != Some(if expected { "true" } else { "false" })
        {
            return Err(format!("unexpected {key} output: {:?}", result.outputs));
        }
    }
    Ok(())
}

pub(super) fn target_dir(project: &Path, store: &Path) -> Result<PathBuf, String> {
    let link = project.join("target");
    if !fs::symlink_metadata(&link)
        .map_err(|err| err.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err(format!("target is not a managed link: {}", link.display()));
    }
    let target = link.canonicalize().map_err(|err| err.to_string())?;
    if !target.starts_with(store) {
        return Err(format!("target escaped store: {}", target.display()));
    }
    Ok(target)
}
