// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

#[test]
fn pack_stops_at_the_first_member_once_the_job_is_cancelled() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("tree"))?;
    fs::write(dir.join("tree/a"), "a")?;
    let cancel = JobCancel::new();
    cancel.cancel();
    let out = dir.join("out.tar");
    let result = pack(&[dir.join("tree")], File::create(&out)?, Some(&cancel));
    let written = fs::metadata(&out).map(|meta| meta.len());
    fs::remove_dir_all(&dir)?;
    assert!(result.is_err_and(|error| error.is::<Cancelled>()));
    assert!(written? <= 1024);
    Ok(())
}

const SPARSE: u64 = 1 << 30;

fn sparse_file(name: &str, size: u64) -> Result<(PathBuf, PathBuf)> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir)?;
    let file = dir.join(name);
    File::create(&file)?.set_len(size)?;
    Ok((dir, file))
}

fn pack_witnessed(
    file: &Path,
    out: &Path,
    cancel: &JobCancel,
    disturb: impl FnOnce() + Send,
) -> (Result<()>, bool) {
    let returned = AtomicBool::new(false);
    let overlapped = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            disturb();
            overlapped.store(!returned.load(Ordering::SeqCst), Ordering::SeqCst);
        });
        let packed = File::create(out)
            .map_err(Into::into)
            .and_then(|out| pack(&[file.to_path_buf()], out, Some(cancel)).map(drop));
        returned.store(true, Ordering::SeqCst);
        packed
    });
    (result, overlapped.load(Ordering::SeqCst))
}

fn pack_while(
    file: &Path,
    out: &Path,
    cancel: &JobCancel,
    disturb: impl FnOnce() + Send,
) -> Result<()> {
    pack_witnessed(file, out, cancel, disturb).0
}

#[test]
fn pack_stops_inside_a_large_file_once_the_job_is_cancelled() -> Result<()> {
    let (dir, file) = sparse_file("big", 9 * SPARSE)?;
    let cancel = JobCancel::new();
    let started = std::time::Instant::now();
    let result = pack_while(&file, &dir.join("out.tar"), &cancel, || cancel.cancel());
    let elapsed = started.elapsed();
    fs::remove_dir_all(&dir)?;
    assert!(result.is_err_and(|error| error.is::<Cancelled>()));
    assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
    Ok(())
}

#[test]
fn pack_refuses_a_file_that_grows_while_it_is_archived() -> Result<()> {
    let (dir, file) = sparse_file("big", SPARSE)?;
    let result = pack_while(&file, Path::new("/dev/null"), &JobCancel::new(), || {
        let _ = fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .and_then(|mut grown| grown.write_all(b"x"));
    });
    fs::remove_dir_all(&dir)?;
    assert!(result.is_err_and(|error| format!("{error:#}").contains("changed while")));
    Ok(())
}

#[test]
fn pack_refuses_a_file_overwritten_in_place_without_changing_size() -> Result<()> {
    let (dir, file) = sparse_file("big", SPARSE)?;
    let result = pack_while(&file, Path::new("/dev/null"), &JobCancel::new(), || {
        let _ = fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .and_then(|mut touched| touched.write_all(b"x"));
    });
    fs::remove_dir_all(&dir)?;
    assert!(result.is_err_and(|error| format!("{error:#}").contains("changed while")));
    Ok(())
}

#[test]
fn pack_refuses_a_file_over_the_entry_limit_without_copying_it() -> Result<()> {
    let (dir, file) = sparse_file("huge", MAX_DATA_BYTES + 1)?;
    let started = std::time::Instant::now();
    let result = pack(&[file], File::create("/dev/null")?, None);
    let elapsed = started.elapsed();
    fs::remove_dir_all(&dir)?;
    assert!(result.is_err_and(|error| format!("{error:#}").contains("10GB limit")));
    assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
    Ok(())
}

#[test]
fn pack_refuses_an_archive_that_holds_none_of_the_declared_paths() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir)?;
    let result = pack(
        &[dir.join("gone")],
        File::create(dir.join("out.tar"))?,
        None,
    );
    fs::remove_dir_all(&dir)?;
    assert!(result.is_err_and(|error| error.to_string().contains("none of the declared")));
    Ok(())
}

#[test]
fn headers_and_padding_count_toward_the_archive_limit() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("tree"))?;
    for index in 0..8 {
        fs::write(dir.join(format!("tree/f{index}")), "")?;
    }
    let out = dir.join("out.tar");
    let within = pack_within(&[dir.join("tree")], File::create(&out)?, None, 1 << 20);
    let over = pack_within(&[dir.join("tree")], File::create(&out)?, None, 4096);
    fs::remove_dir_all(&dir)?;
    within?;
    assert!(over.is_err_and(|error| format!("{error:#}").contains("10GB limit")));
    Ok(())
}

#[test]
fn a_tree_of_many_nested_directories_packs_without_holding_a_descriptor_per_level() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    let mut deep = dir.join("tree");
    for _ in 0..120 {
        deep.push("d");
    }
    fs::create_dir_all(&deep)?;
    fs::write(deep.join("leaf"), "x")?;
    let out = dir.join("out.tar");
    let limit = rustix::process::getrlimit(rustix::process::Resource::Nofile);
    rustix::process::setrlimit(
        rustix::process::Resource::Nofile,
        rustix::process::Rlimit {
            current: Some(64),
            maximum: limit.maximum,
        },
    )?;
    let packed = pack(&[dir.join("tree")], File::create(&out)?, None);
    rustix::process::setrlimit(rustix::process::Resource::Nofile, limit)?;
    let names = tar::Archive::new(File::open(&out)?).entries()?.count();
    fs::remove_dir_all(&dir)?;
    packed?;
    assert_eq!(names, 122);
    Ok(())
}

#[test]
fn pack_refuses_a_directory_that_gains_a_member_while_it_is_archived() -> Result<()> {
    let (dir, _) = sparse_file("big", SPARSE)?;
    let result = pack_while(&dir, Path::new("/dev/null"), &JobCancel::new(), || {
        let _ = fs::write(dir.join("late"), "x");
    });
    fs::remove_dir_all(&dir)?;
    assert!(result.is_err_and(|error| format!("{error:#}").contains("changed while")));
    Ok(())
}

#[test]
fn the_archive_cap_counts_every_write_and_is_inclusive_over_a_multi_megabyte_tree() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("tree"))?;
    for index in 0..6 {
        fs::write(dir.join(format!("tree/f{index}")), vec![index; 300 << 10])?;
    }
    let roots = [dir.join("tree")];
    let mut whole = Vec::new();
    let size = pack_within(&roots, &mut whole, None, u64::MAX)?;
    let exact = pack_within(&roots, io::sink(), None, size);
    let short = pack_within(&roots, io::sink(), None, size - 1);
    fs::remove_dir_all(&dir)?;
    assert!(size > 1 << 20);
    assert_eq!(size, whole.len() as u64);
    assert_eq!(exact?, size);
    assert!(short.is_err_and(|error| format!("{error:#}").contains("over the 10GB limit")));
    Ok(())
}

#[test]
fn a_change_of_ctime_alone_does_not_fail_the_save() -> Result<()> {
    let (dir, file) = sparse_file("big", SPARSE)?;
    let (result, overlapped) =
        pack_witnessed(&file, Path::new("/dev/null"), &JobCancel::new(), || {
            let _ = fs::hard_link(&file, dir.join("link"));
            let _ =
                fs::metadata(&file).and_then(|meta| fs::set_permissions(&file, meta.permissions()));
        });
    let linked = dir.join("link").exists();
    fs::remove_dir_all(&dir)?;
    assert!(linked);
    assert!(overlapped, "the pack returned before the ctime changed");
    result?;
    Ok(())
}

#[test]
fn a_link_replaced_between_the_inspection_and_the_read_is_refused() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir)?;
    symlink("aaaa", dir.join("l"))?;
    let parent = openat(CWD, &dir, directory_flags(), Mode::empty())?;
    let mut packer = Packer::new(Builder::new(io::sink()), None);
    let name = Path::new("l");
    let fresh = statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    let unchanged = packer.link(parent.as_fd(), name, &fresh, Path::new("0/l"));
    let stale = statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
    symlink("bbbb", dir.join("replacement"))?;
    fs::rename(dir.join("replacement"), dir.join("l"))?;
    let replaced = packer.link(parent.as_fd(), name, &stale, Path::new("0/l"));
    fs::remove_dir_all(&dir)?;
    unchanged?;
    assert!(replaced.is_err_and(|error| format!("{error:#}").contains("changed while")));
    Ok(())
}

#[test]
fn hard_links_are_stored_once_and_named_after_the_first_member_like_gnu_tar() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("tree"))?;
    fs::write(dir.join("tree/a"), vec![7_u8; 4096])?;
    fs::hard_link(dir.join("tree/a"), dir.join("tree/b"))?;
    fs::write(dir.join("tree/other"), "o")?;
    let mut archive = Vec::new();
    pack(&[dir.join("tree")], &mut archive, None)?;
    fs::remove_dir_all(&dir)?;
    let mut tar = tar::Archive::new(archive.as_slice());
    let mut regular = Vec::new();
    let mut links = Vec::new();
    for entry in tar.entries()? {
        let entry = entry?;
        let path = entry.path()?.into_owned();
        match entry.header().entry_type() {
            EntryType::Regular => regular.push((path, entry.header().size()?)),
            EntryType::Link => {
                let target = entry.link_name()?.map(std::borrow::Cow::into_owned);
                links.push((path, target, entry.header().size()?));
            }
            _ => {}
        }
    }
    let stored: Vec<_> = regular
        .iter()
        .filter(|(path, _)| path.ends_with("a") || path.ends_with("b"))
        .collect();
    assert_eq!(stored.len(), 1, "{regular:?}");
    assert_eq!(stored[0].1, 4096);
    assert_eq!(links.len(), 1, "{links:?}");
    assert_eq!(links[0].1.as_deref(), Some(stored[0].0.as_path()));
    assert_eq!(links[0].2, 0);
    assert_ne!(links[0].0, stored[0].0);
    Ok(())
}

#[test]
fn a_nested_root_below_a_link_inside_another_root_is_refused_at_save() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("data"))?;
    fs::create_dir_all(dir.join("elsewhere/subdir"))?;
    symlink("../elsewhere", dir.join("data/link"))?;
    let result = pack(
        &[dir.join("data"), dir.join("data/link/subdir")],
        std::io::sink(),
        None,
    );
    fs::remove_dir_all(&dir)?;
    let error = result.err().map(|error| format!("{error:#}"));
    assert!(
        error
            .as_deref()
            .is_some_and(|text| text.contains("never writes through a link")),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn a_nested_root_reached_through_an_alias_of_another_root_is_refused_at_save() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("data"))?;
    fs::create_dir_all(dir.join("elsewhere/subdir"))?;
    symlink("../elsewhere", dir.join("data/link"))?;
    symlink("data", dir.join("alias"))?;
    let result = pack(
        &[dir.join("data"), dir.join("alias/link/subdir")],
        std::io::sink(),
        None,
    );
    fs::remove_dir_all(&dir)?;
    let error = result.err().map(|error| format!("{error:#}"));
    assert!(
        error
            .as_deref()
            .is_some_and(|text| text.contains("never writes through a link")),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn a_nested_root_below_real_directories_inside_another_root_packs() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-pack-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("data/real/subdir"))?;
    let packed = pack(
        &[dir.join("data"), dir.join("data/real/subdir")],
        std::io::sink(),
        None,
    );
    fs::remove_dir_all(&dir)?;
    assert!(packed.is_ok(), "{packed:?}");
    Ok(())
}
