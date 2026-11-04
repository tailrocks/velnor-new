// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

use crate::gc1_mbx_stub::{DF_AVAIL_ENV, TAR_LOG_ENV};
use crate::gc1_scene::{Scene, fail};
use crate::gc1_ustar::{Archive, decode_zstd, list_members};
use crate::state_v4_bounds::{KIB, MEMBERS, bounded, payload_dir, require_imported, require_in};

const DISK_STORED: u64 = 64 * KIB;
const DISK_EXTRACTED: u64 = 8 * 1024 * KIB;
const BLOCK: u64 = 512;
const FILE_BLOCK: u64 = 4 * KIB;
const TINY_MEMBERS: u64 = 200;
const FILE_LIMIT_BLOCKS: u64 = 2 * KIB;
const FILE_LIMIT: u64 = FILE_LIMIT_BLOCKS * BLOCK;

fn decoded_bytes(payload: &Path) -> Result<u64, Box<dyn Error>> {
    let oracle = tempfile::tempdir()?;
    let plain = oracle.path().join("oracle.tar");
    decode_zstd(payload, &plain)?;
    Ok(std::fs::metadata(&plain)?.len())
}

fn aligned_payload(scene: &Scene) -> Result<(), Box<dyn Error>> {
    payload_dir(scene)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    archive.dir("state/cas/pad/")?;
    archive.file("state/cas/pad/a", b"a")?;
    archive.file("state/cas/pad/b", b"b")?;
    archive.finish()?;
    Ok(())
}

fn tiny_payload(scene: &Scene) -> Result<(), Box<dyn Error>> {
    payload_dir(scene)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    archive.dir("state/cas/tiny/")?;
    for index in 0..TINY_MEMBERS {
        archive.file(&format!("state/cas/tiny/t{index}"), b"x")?;
    }
    archive.finish()?;
    Ok(())
}

fn disk_need(payload: &Path) -> Result<(u64, u64), Box<dyn Error>> {
    let plain = decoded_bytes(payload)?;
    let members = u64::try_from(list_members(payload)?.lines().count())?;
    Ok((plain, plain + plain / 8 + members * FILE_BLOCK))
}

fn zero_members(scene: &Scene, count: u64, bytes: u64) -> Result<(), Box<dyn Error>> {
    payload_dir(scene)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    for index in 0..count {
        archive.zeros(&format!("state/cas/bulk{index}"), bytes)?;
    }
    archive.finish()?;
    Ok(())
}

fn extractions(scene: &Scene) -> usize {
    std::fs::read_to_string(scene.scratch.join("tar.log")).map_or(0, |log| log.lines().count())
}

fn run_logged(
    scene: &Scene,
    free_kib: Option<&str>,
    fsize_blocks: Option<u64>,
) -> Result<String, Box<dyn Error>> {
    scene.shim_tar_log()?;
    let key = scene.discover()?;
    let log = scene.scratch.join("tar.log").display().to_string();
    let mut env = vec![(TAR_LOG_ENV, log.as_str())];
    if let Some(free) = free_kib {
        env.push((DF_AVAIL_ENV, free));
    }
    scene.recover(&key.exact(), &env, fsize_blocks)
}

#[test]
fn recover_disk_need_boundary() -> Result<(), Box<dyn Error>> {
    let oracle = bounded("v4-disk-oracle", DISK_STORED, DISK_EXTRACTED, MEMBERS)?;
    aligned_payload(&oracle)?;
    let (plain, need) = disk_need(&oracle.payload())?;
    oracle.finish()?;
    if plain != 8 * KIB || !need.is_multiple_of(KIB) {
        return Err(fail(&format!(
            "the boundary payload lost its alignment: plain={plain} need={need}"
        )));
    }
    let enough_kib = need / KIB;
    let short_kib = enough_kib - 1;

    let scene = bounded("v4-disk-short", DISK_STORED, DISK_EXTRACTED, MEMBERS)?;
    aligned_payload(&scene)?;
    let output = run_logged(&scene, Some(&short_kib.to_string()), None)?;
    require_in(
        &output,
        &format!(
            "velnor: state snapshot cold (disk_free={} need={need})",
            short_kib * KIB
        ),
        "free space one KiB under the need",
    )?;
    scene.assert_silent_cold(&output, "state snapshot cold")?;
    if !Scene::gone(&scene.payload()) {
        return Err(fail("a short disk left the payload"));
    }
    if extractions(&scene) != 0 {
        return Err(fail("a short disk reached the extraction"));
    }
    scene.finish()?;

    let scene = bounded("v4-disk-enough", DISK_STORED, DISK_EXTRACTED, MEMBERS)?;
    aligned_payload(&scene)?;
    run_logged(&scene, Some(&enough_kib.to_string()), None)?;
    require_imported(&scene, "free space equal to the need")?;
    if extractions(&scene) != 1 {
        return Err(fail("the recovery skipped the extraction"));
    }
    scene.finish()
}

#[test]
fn recover_never_writes_the_decoded_archive() -> Result<(), Box<dyn Error>> {
    let member = 512 * KIB;
    let scene = bounded("v4-no-inflated-tar", DISK_STORED, DISK_EXTRACTED, MEMBERS)?;
    zero_members(&scene, 8, member)?;
    if decoded_bytes(&scene.payload())? <= FILE_LIMIT || member >= FILE_LIMIT {
        return Err(fail(
            "the limit no longer separates the archive from a member",
        ));
    }
    run_logged(&scene, None, Some(FILE_LIMIT_BLOCKS))?;
    require_imported(&scene, "archive larger than any file the runner may write")?;
    scene.finish()
}

#[test]
fn recover_goes_cold_when_the_extraction_cannot_write() -> Result<(), Box<dyn Error>> {
    let member = 2 * FILE_LIMIT;
    let scene = bounded("v4-extract-enospc", DISK_STORED, DISK_EXTRACTED, MEMBERS)?;
    zero_members(&scene, 1, member)?;
    let output = run_logged(&scene, None, Some(FILE_LIMIT_BLOCKS))?;
    scene.assert_cold(&output, "payload unreadable")?;
    if extractions(&scene) != 1 {
        return Err(fail("the extraction was never attempted"));
    }
    scene.finish()
}

#[test]
fn recover_disk_need_counts_the_block_of_every_member() -> Result<(), Box<dyn Error>> {
    let oracle = bounded("v4-tiny-oracle", DISK_STORED, DISK_EXTRACTED, MEMBERS)?;
    tiny_payload(&oracle)?;
    let (plain, need) = disk_need(&oracle.payload())?;
    oracle.finish()?;
    let tar_only_kib = (plain + plain / 8).div_ceil(KIB);
    if tar_only_kib * KIB >= need || tar_only_kib * KIB < plain + plain / 8 {
        return Err(fail("the tiny members no longer separate the two needs"));
    }

    let scene = bounded("v4-tiny-short", DISK_STORED, DISK_EXTRACTED, MEMBERS)?;
    tiny_payload(&scene)?;
    let output = run_logged(&scene, Some(&tar_only_kib.to_string()), None)?;
    require_in(
        &output,
        &format!(
            "velnor: state snapshot cold (disk_free={} need={need})",
            tar_only_kib * KIB
        ),
        "free space for the tar but not for the blocks of its members",
    )?;
    scene.assert_silent_cold(&output, "state snapshot cold")?;
    if extractions(&scene) != 0 {
        return Err(fail("a short disk reached the extraction"));
    }
    scene.finish()?;

    let scene = bounded("v4-tiny-enough", DISK_STORED, DISK_EXTRACTED, MEMBERS)?;
    tiny_payload(&scene)?;
    run_logged(&scene, Some(&need.div_ceil(KIB).to_string()), None)?;
    require_imported(
        &scene,
        "free space for the tar and the blocks of its members",
    )?;
    scene.finish()
}
