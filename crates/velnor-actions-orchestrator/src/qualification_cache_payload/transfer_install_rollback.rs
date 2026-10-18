use super::super::cleanup::remove_owned_at;
use super::super::dirs::{open_directory_at, same_object, stat_optional};
use super::{CreatedPayload, PreparedRoot};
use rustix::fs::{FileType, Mode, OFlags, RenameFlags, Stat, mkdirat, openat, renameat_with};

pub(super) fn rollback_prepared(prepared: &mut [PreparedRoot]) -> Result<(), &'static str> {
    for item in prepared.iter_mut().rev() {
        if item.backup_moved {
            restore_backup(item)?;
        } else if item.replacement_installed {
            remove_installed_replacement(item)?;
        }
    }
    Ok(())
}

fn restore_backup(item: &mut PreparedRoot) -> Result<(), &'static str> {
    let previous = item
        .previous
        .ok_or("qualification_cache_import_rollback_failed")?;
    let backup = stat_optional(&item.parent, item.backup_name.as_c_str())?
        .ok_or("qualification_cache_import_rollback_failed")?;
    if !same_object(&previous, &backup) {
        return Err("qualification_cache_import_rollback_failed");
    }

    if item.replacement_installed {
        restore_backup_over_replacement(item, &previous)
    } else {
        restore_backup_to_empty_destination(item, &previous)
    }
}

fn restore_backup_over_replacement(
    item: &mut PreparedRoot,
    previous: &Stat,
) -> Result<(), &'static str> {
    let temporary = item
        .temporary
        .as_ref()
        .ok_or("qualification_cache_import_rollback_failed")?;
    let destination = stat_optional(&item.parent, item.destination_name.as_c_str())?
        .ok_or("qualification_cache_import_rollback_failed")?;
    if !same_object(&temporary.stat, &destination) {
        return Err("qualification_cache_import_rollback_failed");
    }

    renameat_with(
        &item.parent,
        item.backup_name.as_c_str(),
        &item.parent,
        item.destination_name.as_c_str(),
        RenameFlags::EXCHANGE,
    )
    .map_err(|_| "qualification_cache_import_rollback_failed")?;

    let restored = stat_optional(&item.parent, item.destination_name.as_c_str())?
        .ok_or("qualification_cache_import_rollback_failed")?;
    let displaced = stat_optional(&item.parent, item.backup_name.as_c_str())?
        .ok_or("qualification_cache_import_rollback_failed")?;
    if !same_object(previous, &restored) || !same_object(&temporary.stat, &displaced) {
        if same_object(&temporary.stat, &displaced) {
            renameat_with(
                &item.parent,
                item.backup_name.as_c_str(),
                &item.parent,
                item.destination_name.as_c_str(),
                RenameFlags::EXCHANGE,
            )
            .map_err(|_| "qualification_cache_import_rollback_failed")?;
        }
        return Err("qualification_cache_import_rollback_failed");
    }

    remove_owned_at(&item.parent, item.backup_name.as_c_str(), temporary)?;
    item.replacement_installed = false;
    item.backup_moved = false;
    Ok(())
}

fn restore_backup_to_empty_destination(
    item: &mut PreparedRoot,
    previous: &Stat,
) -> Result<(), &'static str> {
    if stat_optional(&item.parent, item.destination_name.as_c_str())?.is_some() {
        return Err("qualification_cache_import_rollback_failed");
    }

    let placeholder = create_rollback_placeholder(item, previous)?;
    if renameat_with(
        &item.parent,
        item.backup_name.as_c_str(),
        &item.parent,
        item.destination_name.as_c_str(),
        RenameFlags::EXCHANGE,
    )
    .is_err()
    {
        remove_owned_at(&item.parent, item.destination_name.as_c_str(), &placeholder)?;
        return Err("qualification_cache_import_rollback_failed");
    }

    let restored = stat_optional(&item.parent, item.destination_name.as_c_str())?;
    let displaced = stat_optional(&item.parent, item.backup_name.as_c_str())?;
    if restored
        .as_ref()
        .is_some_and(|stat| same_object(previous, stat))
        && displaced
            .as_ref()
            .is_some_and(|stat| same_object(&placeholder.stat, stat))
    {
        remove_owned_at(&item.parent, item.backup_name.as_c_str(), &placeholder)?;
        item.backup_moved = false;
        return Ok(());
    }

    if displaced
        .as_ref()
        .is_some_and(|stat| same_object(&placeholder.stat, stat))
    {
        renameat_with(
            &item.parent,
            item.backup_name.as_c_str(),
            &item.parent,
            item.destination_name.as_c_str(),
            RenameFlags::EXCHANGE,
        )
        .map_err(|_| "qualification_cache_import_rollback_failed")?;
        remove_owned_at(&item.parent, item.destination_name.as_c_str(), &placeholder)?;
    } else if stat_optional(&item.parent, item.destination_name.as_c_str())?
        .is_some_and(|stat| same_object(&placeholder.stat, &stat))
    {
        remove_owned_at(&item.parent, item.destination_name.as_c_str(), &placeholder)?;
    }
    Err("qualification_cache_import_rollback_failed")
}

fn create_rollback_placeholder(
    item: &PreparedRoot,
    previous: &Stat,
) -> Result<CreatedPayload, &'static str> {
    let descriptor = match FileType::from_raw_mode(previous.st_mode) {
        FileType::Directory => {
            mkdirat(
                &item.parent,
                item.destination_name.as_c_str(),
                Mode::RUSR | Mode::WUSR | Mode::XUSR,
            )
            .map_err(|_| "qualification_cache_import_rollback_failed")?;
            open_directory_at(&item.parent, item.destination_name.as_c_str())?
        }
        FileType::RegularFile => openat(
            &item.parent,
            item.destination_name.as_c_str(),
            OFlags::CREATE | OFlags::EXCL | OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|_| "qualification_cache_import_rollback_failed")?,
        _ => return Err("qualification_cache_import_rollback_failed"),
    };
    let stat =
        rustix::fs::fstat(&descriptor).map_err(|_| "qualification_cache_import_rollback_failed")?;
    let path_stat = stat_optional(&item.parent, item.destination_name.as_c_str())?
        .ok_or("qualification_cache_import_rollback_failed")?;
    if !same_object(&stat, &path_stat) {
        return Err("qualification_cache_import_rollback_failed");
    }
    Ok(CreatedPayload {
        digest: String::new(),
        descriptor,
        stat,
    })
}

fn remove_installed_replacement(item: &mut PreparedRoot) -> Result<(), &'static str> {
    let current = stat_optional(&item.parent, item.destination_name.as_c_str())?
        .ok_or("qualification_cache_import_rollback_failed")?;
    let temporary = item
        .temporary
        .as_ref()
        .ok_or("qualification_cache_import_rollback_failed")?;
    if !same_object(&temporary.stat, &current) {
        return Err("qualification_cache_import_rollback_failed");
    }
    remove_owned_at(&item.parent, item.destination_name.as_c_str(), temporary)?;
    item.replacement_installed = false;
    Ok(())
}
