// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use crate::gc1_scene::{Scene, files_under};
use crate::gc1_ustar::Archive;

use super::*;

const DEPTH_LIMIT: usize = 64;
const NAME_LIMIT: usize = 255;
const V3_DIRECTORIES: usize = 1500;
const BASE: &str = "state/cas";

#[derive(Clone, Copy, Debug)]
enum Bottom {
    File,
    Directory,
}

struct Shape {
    components: usize,
    longest: usize,
}

fn build(
    scene: &Scene,
    components: usize,
    name: &str,
    mode: u64,
    bottom: Bottom,
) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(scene.payload().parent().ok_or_else(|| fail("no parent"))?)?;
    let mut archive = Archive::create(&scene.payload())?;
    archive.state_layout("old")?;
    let mut path = BASE.to_owned();
    let below = components - 2;
    let dirs = match bottom {
        Bottom::File => below - 1,
        Bottom::Directory => below,
    };
    for _ in 0..dirs {
        path = format!("{path}/{name}");
        archive.dir_mode(&format!("{path}/"), mode)?;
    }
    if let Bottom::File = bottom {
        archive.file(&format!("{path}/f"), b"deep")?;
    }
    archive.finish()?;
    Ok(())
}

fn listed(payload: &Path) -> Result<Shape, Box<dyn Error>> {
    let output = Command::new("tar").arg("-tzf").arg(payload).output()?;
    if !output.status.success() {
        return Err(fail("the system tar cannot list the fixture"));
    }
    let text = String::from_utf8(output.stdout)?;
    let mut shape = Shape {
        components: 0,
        longest: 0,
    };
    for line in text.lines() {
        let parts: Vec<&str> = line.trim_end_matches('/').split('/').collect();
        shape.components = shape.components.max(parts.len());
        shape.longest = shape
            .longest
            .max(parts.iter().map(|part| part.len()).max().unwrap_or(0));
    }
    Ok(shape)
}

fn expect_shape(scene: &Scene, components: usize, longest: usize) -> Result<(), Box<dyn Error>> {
    let shape = listed(&scene.payload())?;
    if shape.components != components || shape.longest < longest {
        return Err(fail(&format!(
            "fixture is vacuous: {} components and a {} byte name, wanted {components} and {longest}",
            shape.components, shape.longest
        )));
    }
    Ok(())
}

fn check_refused(
    label: &str,
    components: usize,
    name: &str,
    mode: u64,
    bottom: Bottom,
    token: &str,
) -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("depth-refused", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    build(&scene, components, name, mode, bottom)?;
    expect_shape(&scene, components, name.len())?;
    let before = scene.tree()?;
    let output = scene
        .recover(&key.key, &key.key, &[], None)
        .map_err(|error| fail(&format!("{label}: {error}")))?;
    scene
        .assert_cold(&output, token)
        .map_err(|error| fail(&format!("{label}: {error}")))?;
    scene
        .assert_confined(&before)
        .map_err(|error| fail(&format!("{label}: {error}")))?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_accepts_a_member_path_at_the_depth_limit() -> Result<(), Box<dyn Error>> {
    let scene = Scene::new("depth-accepted", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    build(&scene, DEPTH_LIMIT, "d", 0o755, Bottom::File)?;
    expect_shape(&scene, DEPTH_LIMIT, 1)?;
    let before = scene.tree()?;
    let output = scene.recover(&key.key, &key.key, &[], None)?;
    if output.contains("::warning::") || !output.contains("recovered") {
        return Err(fail(&format!("recover refused a legal depth: {output}")));
    }
    let deepest = files_under(&scene.store())?
        .iter()
        .map(|file| file.components().count())
        .max()
        .unwrap_or(0);
    let base = scene.store().components().count();
    if deepest < base + DEPTH_LIMIT - 2 {
        return Err(fail("the store lost the deepest member"));
    }
    scene.assert_clean_exit()?;
    scene.assert_confined_recovered(&before)?;
    scene.finish()
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_a_member_file_one_past_the_depth_limit() -> Result<(), Box<dyn Error>> {
    check_refused(
        "file",
        DEPTH_LIMIT + 1,
        "d",
        0o755,
        Bottom::File,
        "refused-depth",
    )
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_a_directory_chain_one_past_the_depth_limit() -> Result<(), Box<dyn Error>>
{
    check_refused(
        "directory",
        DEPTH_LIMIT + 1,
        "d",
        0o755,
        Bottom::Directory,
        "refused-depth",
    )
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_a_chain_of_directories_without_search_bits() -> Result<(), Box<dyn Error>>
{
    check_refused(
        "1500 directories",
        V3_DIRECTORIES + 2,
        "d",
        0o644,
        Bottom::File,
        "refused-depth",
    )
}

#[test]
#[cfg(unix)]
fn gc1_t53_recover_refuses_a_member_name_past_the_name_limit() -> Result<(), Box<dyn Error>> {
    let name = "n".repeat(NAME_LIMIT + 1);
    check_refused("long name", 4, &name, 0o755, Bottom::File, "refused-name")
}
