// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::gc1_mbx_stub::{TAR_LIST_LOG_ENV, TAR_LOG_ENV};
use crate::gc1_scene::Scene;
use crate::gc1_ustar::{
    Archive, decode_zstd, long_window_frame, skippable_only, skippable_prefix, truncate_to,
    wrap_zstd,
};

use super::*;

const SHADOW: &str = "refused-shadow";
const UNREADABLE: &str = "payload unreadable";
const NO_FILE: &str = "no regular file";
const FORMAT: &str = "refused-format";
const MEMBER_BOUND: u32 = 100_000;
const ZERO_TAR: u64 = 10 * 1024;

#[derive(Clone, Copy, Debug)]
enum Row {
    SiblingFamily,
    ParentTraversal,
    SymlinkWriteThrough,
    HardLink,
    NestedGzipInZstd,
    NestedZstdInZstd,
    EmptyTar,
    ZeroTar,
    NewlineName,
    AbsoluteName,
    Fifo,
    NotZstd,
    PolyglotSkippable,
    SkippableOnly,
    TruncatedFrame,
    LongWindow31,
    OuterGzip,
    TooManyMembers,
    V7Tar,
    BeyondCalendarName,
    DirsOnly,
    TruncatedTar,
    ListingDisagrees,
    NamesListingFails,
    TypesListingFails,
}

const ROWS: [Row; 25] = [
    Row::SiblingFamily,
    Row::ParentTraversal,
    Row::SymlinkWriteThrough,
    Row::HardLink,
    Row::NestedGzipInZstd,
    Row::NestedZstdInZstd,
    Row::EmptyTar,
    Row::ZeroTar,
    Row::NewlineName,
    Row::AbsoluteName,
    Row::Fifo,
    Row::NotZstd,
    Row::PolyglotSkippable,
    Row::SkippableOnly,
    Row::TruncatedFrame,
    Row::LongWindow31,
    Row::OuterGzip,
    Row::TooManyMembers,
    Row::V7Tar,
    Row::BeyondCalendarName,
    Row::DirsOnly,
    Row::TruncatedTar,
    Row::ListingDisagrees,
    Row::NamesListingFails,
    Row::TypesListingFails,
];

fn gated(row: Row) -> bool {
    matches!(
        row,
        Row::NestedGzipInZstd
            | Row::NestedZstdInZstd
            | Row::EmptyTar
            | Row::ZeroTar
            | Row::V7Tar
            | Row::LongWindow31
    )
}

fn token(row: Row) -> &'static str {
    match row {
        Row::NotZstd | Row::SkippableOnly | Row::OuterGzip => FORMAT,
        Row::TruncatedFrame
        | Row::LongWindow31
        | Row::TruncatedTar
        | Row::NamesListingFails
        | Row::TypesListingFails => UNREADABLE,
        Row::DirsOnly => NO_FILE,
        _ => SHADOW,
    }
}

fn absolute_target() -> String {
    format!("/tmp/velnor-gc1-absolute-{}/pwn", std::process::id())
}

fn build_wrapped(row: Row, scene: &Scene) -> Result<(), Box<dyn Error>> {
    let payload = scene.payload();
    let inner = scene.scratch.join("inner");
    let mut archive = match row {
        Row::NestedGzipInZstd => Archive::create_gzip(&inner)?,
        _ => Archive::create(&inner)?,
    };
    archive.state_layout("nested")?;
    archive.finish()?;
    if let Row::PolyglotSkippable = row {
        let patched = scene.scratch.join("patched");
        skippable_prefix(&inner, &patched)?;
        return wrap_zstd(&patched, &payload);
    }
    wrap_zstd(&inner, &payload)
}

fn build_raw(row: Row, scene: &Scene) -> Result<(), Box<dyn Error>> {
    let payload = scene.payload();
    match row {
        Row::NotZstd => Ok(std::fs::write(
            &payload,
            b"this payload was never a zstd stream\n",
        )?),
        Row::SkippableOnly => skippable_only(&payload),
        Row::LongWindow31 => long_window_frame(&payload),
        Row::OuterGzip => {
            let mut archive = Archive::create_gzip(&payload)?;
            archive.state_layout("old")?;
            archive.finish()?;
            Ok(())
        }
        Row::TruncatedFrame => {
            let whole = scene.scratch.join("whole");
            let mut archive = Archive::create(&whole)?;
            archive.state_layout("old")?;
            archive.finish()?;
            let half = usize::try_from(std::fs::metadata(&whole)?.len() / 2)?;
            truncate_to(&whole, &payload, half)
        }
        _ => Err(fail("row is not a raw payload")),
    }
}

fn build_bare(row: Row, scene: &Scene) -> Result<(), Box<dyn Error>> {
    let mut archive = Archive::create(&scene.payload())?;
    match row {
        Row::EmptyTar => {}
        Row::ZeroTar => archive.raw_zeros(ZERO_TAR)?,
        _ => return Err(fail("row is not a bare stream")),
    }
    archive.finish_untrailed()?;
    Ok(())
}

fn build_v7(scene: &Scene) -> Result<(), Box<dyn Error>> {
    let mut archive = Archive::create_v7(&scene.payload())?;
    archive.state_layout("old")?;
    archive.finish()?;
    Ok(())
}

fn build_members(row: Row, scene: &Scene, outside: &Path) -> Result<(), Box<dyn Error>> {
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    match row {
        Row::SiblingFamily => {
            archive.dir("tools/")?;
            archive.file("tools/sentinel", b"must never land")?;
        }
        Row::ParentTraversal => archive.file("state/../escape", b"must never land")?,
        Row::SymlinkWriteThrough => {
            archive.symlink("state/lnk", &outside.display().to_string())?;
            archive.file("state/lnk/pwn", b"must never land")?;
        }
        Row::HardLink => {
            archive.file("state/obj", b"linked")?;
            archive.hardlink("state/hl", "state/obj")?;
        }
        Row::NewlineName => archive.file("state/a\nb", b"must never land")?,
        Row::BeyondCalendarName => {
            archive.file_beyond_calendar("zz state/f", b"must never land")?;
        }
        Row::AbsoluteName => archive.file(&absolute_target(), b"must never land")?,
        Row::Fifo => archive.fifo("state/pipe")?,
        Row::TooManyMembers => {
            for index in 0..=MEMBER_BOUND {
                archive.file(&format!("state/cas/f{index}"), b"")?;
            }
        }
        _ => return Err(fail("row is not a member stream")),
    }
    archive.finish()?;
    Ok(())
}

fn build_dirs(scene: &Scene) -> Result<(), Box<dyn Error>> {
    let mut archive = Archive::create(&scene.payload())?;
    archive.dir("state/")?;
    archive.dir("state/cas/")?;
    archive.finish()?;
    Ok(())
}

fn build(row: Row, scene: &Scene, outside: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    match row {
        Row::NestedGzipInZstd | Row::NestedZstdInZstd | Row::PolyglotSkippable => {
            build_wrapped(row, scene)
        }
        Row::EmptyTar | Row::ZeroTar => build_bare(row, scene),
        Row::V7Tar => build_v7(scene),
        Row::DirsOnly => build_dirs(scene),
        Row::TruncatedTar => {
            let mut archive = Archive::create(&scene.payload())?;
            archive.state_layout("old")?;
            archive.file("state/cas/big", &vec![7u8; 100 * 1024])?;
            archive.finish()?;
            let plain = scene.scratch.join("plain.tar");
            let cut = scene.scratch.join("cut.tar");
            decode_zstd(&scene.payload(), &plain)?;
            let keep = usize::try_from(std::fs::metadata(&plain)?.len() * 6 / 10)?;
            truncate_to(&plain, &cut, keep)?;
            wrap_zstd(&cut, &scene.payload())
        }
        Row::NamesListingFails | Row::TypesListingFails => {
            let mut archive = Archive::create(&scene.payload())?;
            archive.state_layout("old")?;
            archive.finish()?;
            Ok(())
        }
        Row::ListingDisagrees => {
            let mut archive = Archive::create(&scene.payload())?;
            archive.state_layout("old")?;
            archive.symlink("state/lnk", "x")?;
            archive.finish()?;
            Ok(())
        }
        Row::NotZstd
        | Row::SkippableOnly
        | Row::TruncatedFrame
        | Row::LongWindow31
        | Row::OuterGzip => build_raw(row, scene),
        _ => build_members(row, scene, outside),
    }
}

fn check_row(row: Row) -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("hostile", crate::gc1_t53::scripts()?)?;
    let outside = scene.scratch.join("outside");
    std::fs::create_dir_all(&outside)?;
    let key = scene.discover()?;
    build(row, &scene, &outside)?;
    let extracts = matches!(row, Row::DirsOnly);
    if !extracts {
        scene.shim_tar_log()?;
    }
    if matches!(row, Row::ListingDisagrees) {
        let real = std::env::var("PATH")?
            .split(':')
            .map(|d| format!("{d}/tar"))
            .find(|p| Path::new(p).is_file())
            .ok_or_else(|| fail("no tar"))?;
        scene.install_shim("tar", &format!("#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in -xf) printf 'extract\\n' >> \"$VELNOR_STUB_TAR_LOG\" ;; esac; done\ncase \" $* \" in *\" -tvf \"*) {real} \"$@\" | sed '$d' ;; *) exec {real} \"$@\" ;; esac\n"))?;
    }
    if let Row::NamesListingFails | Row::TypesListingFails = row {
        let real = std::env::var("PATH")?
            .split(':')
            .map(|d| format!("{d}/tar"))
            .find(|p| Path::new(p).is_file())
            .ok_or_else(|| fail("no tar"))?;
        let mode = if matches!(row, Row::NamesListingFails) {
            "-tf"
        } else {
            "-tvf"
        };
        scene.install_shim("tar", &format!("#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in -xf) printf 'extract\\n' >> \"$VELNOR_STUB_TAR_LOG\" ;; esac; done\ncase \" $* \" in *\" {mode} \"*) {real} \"$@\"; exit 2 ;; *) exec {real} \"$@\" ;; esac\n"))?;
    }
    let before = scene.tree()?;
    let log = scene.scratch.join("tar.log").display().to_string();
    let listing = scene.scratch.join("tar.list").display().to_string();
    let mut env = vec![(TAR_LOG_ENV, log.as_str())];
    if gated(row) {
        env.push((TAR_LIST_LOG_ENV, listing.as_str()));
    }
    let output = scene
        .recover(&key.exact(), &env, None)
        .map_err(|error| fail(&format!("{row:?}: {error}")))?;
    scene
        .assert_cold(&output, token(row))
        .map_err(|error| fail(&format!("{row:?}: {error}")))?;
    scene
        .assert_confined(&before)
        .map_err(|error| fail(&format!("{row:?}: {error}")))?;
    if Path::new(&absolute_target()).exists() {
        return Err(fail(&format!("{row:?}: wrote to an absolute path")));
    }
    if gated(row) && scene.scratch.join("tar.list").exists() {
        return Err(fail(&format!(
            "{row:?}: listed before the first-header gate"
        )));
    }
    if !extracts && scene.scratch.join("tar.log").exists() {
        return Err(fail(&format!("{row:?}: reached the extraction")));
    }
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_hostile_archives() -> Result<(), Box<dyn Error>> {
    let failures: Vec<String> = ROWS
        .into_iter()
        .filter_map(|row| check_row(row).err().map(|error| error.to_string()))
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(fail(&failures.join(" | ")))
    }
}

#[test]
#[cfg(target_os = "linux")]
fn gc1_t53_recover_imports_names_that_look_like_columns() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("lookalike", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    let names = [
        "state/cas/2026-01-01 00:00 x",
        "state/cas/with space",
        "state/cas/ led",
        "state/cas/trail ",
        "state/cas/a 2026-01-01 00:00 x",
    ];
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    for name in names {
        archive.file(name, b"kept")?;
    }
    archive.finish()?;
    let output = scene.recover(&key.exact(), &[], None)?;
    let record = std::fs::read_to_string(scene.recovery_record())
        .map_err(|error| fail(&format!("recover wrote no record ({error}): {output}")))?;
    if !record.contains("decision=exact-hit") {
        return Err(fail(&format!(
            "recover refused the names: {record}: {output}"
        )));
    }
    for name in names {
        let landed = scene.store().join(name.trim_start_matches("state/"));
        let content = std::fs::read_to_string(&landed)
            .map_err(|error| fail(&format!("the store lost {name:?} ({error}): {output}")))?;
        if content != "kept" {
            return Err(fail(&format!("the store changed {name:?}: {output}")));
        }
    }
    scene.finish()
}
