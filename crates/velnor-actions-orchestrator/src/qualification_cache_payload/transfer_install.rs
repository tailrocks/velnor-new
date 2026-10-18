use super::super::manifest::PayloadRoot;
use super::super::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use super::cleanup::{remove_owned_at, remove_tree_at};
use super::dirs::{
    c_name, ensure_owned_directory_path, ensure_owned_directory_path_from, open_owned_directory,
    same_object, same_version, stat_optional,
};
use super::engine::{snapshot_roots, useful_state_digest};
use super::files::copy_file_to_new_owned;
use super::payload;
use super::{CreatedPayload, ROOTS_DIRECTORY, is_file_root, root_name};
use rustix::fs::{FileType, Stat, renameat};
use std::ffi::CString;
use std::path::{Path, PathBuf};

#[path = "transfer_install_rollback.rs"]
mod rollback;
use rollback::rollback_prepared;

#[derive(Debug)]
struct PreparedRoot {
    parent: rustix::fd::OwnedFd,
    destination_name: CString,
    temporary_name: CString,
    backup_name: CString,
    previous: Option<Stat>,
    temporary: Option<CreatedPayload>,
    backup_moved: bool,
    replacement_installed: bool,
}

pub(super) fn install_roots(
    stage_path: &Path,
    roots: &CacheRootSet,
    runner_temp: &Path,
    payload_roots: &[PayloadRoot],
    expected_state: &str,
) -> Result<(), &'static str> {
    install_roots_with_home(
        stage_path,
        roots,
        runner_temp,
        None,
        payload_roots,
        expected_state,
    )
}

pub(super) fn install_normal_tools_roots(
    stage_path: &Path,
    roots: &CacheRootSet,
    runner_temp: &Path,
    home: &Path,
    payload_roots: &[PayloadRoot],
    expected_state: &str,
) -> Result<(), &'static str> {
    install_roots_with_home(
        stage_path,
        roots,
        runner_temp,
        Some(home),
        payload_roots,
        expected_state,
    )
}

fn install_roots_with_home(
    stage_path: &Path,
    roots: &CacheRootSet,
    runner_temp: &Path,
    normal_home: Option<&Path>,
    payload_roots: &[PayloadRoot],
    expected_state: &str,
) -> Result<(), &'static str> {
    let mut prepared = Vec::with_capacity(roots.roots.len());
    for (index, (root, payload_root)) in roots.roots.iter().zip(payload_roots).enumerate() {
        match prepare_root(
            stage_path,
            runner_temp,
            normal_home,
            root,
            payload_root,
            index,
        ) {
            Ok(item) => prepared.push(item),
            Err(error) => {
                cleanup_prepared(&prepared)?;
                return Err(error);
            }
        }
    }
    if let Err(error) = commit_prepared(&mut prepared) {
        rollback_prepared(&mut prepared)?;
        cleanup_prepared(&prepared)?;
        return Err(error);
    }
    let actual = snapshot_roots(roots).and_then(|values| useful_state_digest(&values));
    if actual.as_deref() != Ok(expected_state) {
        rollback_prepared(&mut prepared)?;
        return Err("qualification_cache_import_state_mismatch");
    }
    for item in &mut prepared {
        if item.backup_moved {
            let previous = item
                .previous
                .as_ref()
                .ok_or("qualification_cache_import_rollback_state_invalid")?;
            remove_tree_at(&item.parent, &item.backup_name, previous)?;
            item.backup_moved = false;
        }
    }
    cleanup_prepared(&prepared)?;
    Ok(())
}

fn prepare_root(
    stage_path: &Path,
    runner_temp: &Path,
    normal_home: Option<&Path>,
    root: &CacheRootObservation,
    payload_root: &PayloadRoot,
    index: usize,
) -> Result<PreparedRoot, &'static str> {
    let parent_path = root
        .path
        .parent()
        .ok_or("qualification_cache_root_path_invalid")?;
    if root.id == CacheRootId::MiseData
        && let Some(home) = normal_home
    {
        if parent_path != home.join(".local/share") {
            return Err("qualification_cache_root_path_invalid");
        }
        ensure_owned_directory_path_from(parent_path, home)?;
    } else {
        ensure_owned_directory_path(parent_path, runner_temp)?;
    }
    let parent = open_owned_directory(parent_path)?;
    let destination_os = root
        .path
        .file_name()
        .ok_or("qualification_cache_root_path_invalid")?;
    let destination_name = c_name(destination_os)?;
    let temporary_name = CString::new(format!(
        ".velnor-cache-import-{}-{index}",
        std::process::id()
    ))
    .map_err(|_| "qualification_cache_import_path_invalid")?;
    let backup_name = CString::new(format!(
        ".velnor-cache-backup-{}-{index}",
        std::process::id()
    ))
    .map_err(|_| "qualification_cache_import_path_invalid")?;
    if stat_optional(&parent, temporary_name.as_c_str())?.is_some()
        || stat_optional(&parent, backup_name.as_c_str())?.is_some()
    {
        return Err("qualification_cache_import_temporary_exists");
    }
    let previous = stat_optional(&parent, destination_name.as_c_str())?;
    if let Some(stat) = &previous {
        validate_destination_root(stat, root.id)?;
    }
    let temporary = if payload_root.present {
        let source = PathBuf::from(stage_path)
            .join(ROOTS_DIRECTORY)
            .join(root_name(root.id));
        let temporary_text = temporary_name
            .to_str()
            .map_err(|_| "qualification_cache_import_path_invalid")?;
        let destination = parent_path.join(temporary_text);
        let created = if is_file_root(root.id) {
            copy_file_to_new_owned(&source, &destination)?
        } else {
            payload::copy_tree_to_new_dir_owned(&source, &destination)?
        };
        if payload_root.digest.as_deref() != Some(created.digest.as_str()) {
            remove_owned_at(&parent, temporary_name.as_c_str(), &created)?;
            return Err("qualification_cache_import_copy_mismatch");
        }
        Some(created)
    } else {
        None
    };
    Ok(PreparedRoot {
        parent,
        destination_name,
        temporary_name,
        backup_name,
        previous,
        temporary,
        backup_moved: false,
        replacement_installed: false,
    })
}

fn commit_prepared(prepared: &mut [PreparedRoot]) -> Result<(), &'static str> {
    for item in prepared {
        if let Some(previous) = &item.previous {
            let current = stat_optional(&item.parent, item.destination_name.as_c_str())?
                .ok_or("qualification_cache_import_destination_changed")?;
            if !same_version(previous, &current) {
                return Err("qualification_cache_import_destination_changed");
            }
            renameat(
                &item.parent,
                item.destination_name.as_c_str(),
                &item.parent,
                item.backup_name.as_c_str(),
            )
            .map_err(|_| "qualification_cache_import_replace_failed")?;
            item.backup_moved = true;
        } else if stat_optional(&item.parent, item.destination_name.as_c_str())?.is_some() {
            return Err("qualification_cache_import_destination_changed");
        }
        if let Some(temporary) = &item.temporary {
            renameat(
                &item.parent,
                item.temporary_name.as_c_str(),
                &item.parent,
                item.destination_name.as_c_str(),
            )
            .map_err(|_| "qualification_cache_import_replace_failed")?;
            item.replacement_installed = true;
            let current = stat_optional(&item.parent, item.destination_name.as_c_str())?
                .ok_or("qualification_cache_import_destination_changed")?;
            if !same_object(&temporary.stat, &current) {
                return Err("qualification_cache_import_destination_changed");
            }
        }
    }
    Ok(())
}

fn cleanup_prepared(prepared: &[PreparedRoot]) -> Result<(), &'static str> {
    for item in prepared {
        if let Some(temporary) = &item.temporary {
            remove_owned_at(&item.parent, item.temporary_name.as_c_str(), temporary)?;
        }
        if item.backup_moved {
            return Err("qualification_cache_import_backup_leftover");
        }
    }
    Ok(())
}

fn validate_destination_root(stat: &Stat, id: CacheRootId) -> Result<(), &'static str> {
    if stat.st_uid != crate::process_owner::effective_uid()
        || stat.st_mode & 0o7000 != 0
        || FileType::from_raw_mode(stat.st_mode)
            != if is_file_root(id) {
                FileType::RegularFile
            } else {
                FileType::Directory
            }
    {
        return Err("qualification_cache_import_destination_invalid");
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/dispatch_cache_transfer_install_tests.rs"]
mod tests;
