use super::dirs::{open_directory_at, same_object, stat_optional};
use super::stage::read_names;
use super::{MAX_DEPTH, MAX_ENTRIES};
use rustix::fs::{AtFlags, FileType, Stat, unlinkat};
use std::ffi::CString;

pub(super) fn remove_tree_at(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
    expected: &Stat,
) -> Result<(), &'static str> {
    let Some(current) = stat_optional(parent, name)? else {
        return Ok(());
    };
    if !same_object(expected, &current) {
        return Err("qualification_cache_remove_changed");
    }
    if FileType::from_raw_mode(current.st_mode) == FileType::Directory {
        let directory = open_directory_at(parent, name)?;
        if !same_object(
            expected,
            &rustix::fs::fstat(&directory).map_err(|_| "qualification_cache_remove_failed")?,
        ) {
            return Err("qualification_cache_remove_changed");
        }
        remove_directory_contents(&directory, 0)?;
        let current = stat_optional(parent, name)?.ok_or("qualification_cache_remove_changed")?;
        if !same_object(expected, &current) {
            return Err("qualification_cache_remove_changed");
        }
        unlinkat(parent, name, AtFlags::REMOVEDIR).map_err(|_| "qualification_cache_remove_failed")
    } else {
        unlinkat(parent, name, AtFlags::empty()).map_err(|_| "qualification_cache_remove_failed")
    }
}

pub(super) fn remove_owned_at(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
    owned: &super::CreatedPayload,
) -> Result<(), &'static str> {
    remove_created_at(parent, name, &owned.descriptor, &owned.stat)
}

pub(super) fn remove_created_at(
    parent: &rustix::fd::OwnedFd,
    name: &std::ffi::CStr,
    descriptor: &rustix::fd::OwnedFd,
    expected: &Stat,
) -> Result<(), &'static str> {
    let descriptor_stat =
        rustix::fs::fstat(descriptor).map_err(|_| "qualification_cache_remove_changed")?;
    if !same_object(expected, &descriptor_stat) {
        return Err("qualification_cache_remove_changed");
    }
    let Some(current) = stat_optional(parent, name)? else {
        return Ok(());
    };
    if !same_object(expected, &current) {
        return Err("qualification_cache_remove_changed");
    }
    remove_tree_at(parent, name, &current)
}

pub(super) fn remove_directory_contents(
    directory: &rustix::fd::OwnedFd,
    depth: usize,
) -> Result<(), &'static str> {
    if depth > MAX_DEPTH {
        return Err("qualification_cache_payload_limit");
    }
    let names = read_names(directory)?;
    if names.len() > MAX_ENTRIES {
        return Err("qualification_cache_payload_limit");
    }
    for text in names {
        let name = CString::new(text).map_err(|_| "qualification_cache_payload_path_invalid")?;
        let Some(before) = stat_optional(directory, name.as_c_str())? else {
            continue;
        };
        if FileType::from_raw_mode(before.st_mode) == FileType::Directory {
            let child = open_directory_at(directory, name.as_c_str())?;
            if !same_object(
                &before,
                &rustix::fs::fstat(&child).map_err(|_| "qualification_cache_remove_failed")?,
            ) {
                return Err("qualification_cache_remove_changed");
            }
            remove_directory_contents(&child, depth + 1)?;
            if stat_optional(directory, name.as_c_str())?
                .is_some_and(|current| same_object(&before, &current))
            {
                unlinkat(directory, name.as_c_str(), AtFlags::REMOVEDIR)
                    .map_err(|_| "qualification_cache_remove_failed")?;
            } else {
                return Err("qualification_cache_remove_changed");
            }
        } else if stat_optional(directory, name.as_c_str())?
            .is_some_and(|current| same_object(&before, &current))
        {
            unlinkat(directory, name.as_c_str(), AtFlags::empty())
                .map_err(|_| "qualification_cache_remove_failed")?;
        } else {
            return Err("qualification_cache_remove_changed");
        }
    }
    Ok(())
}
