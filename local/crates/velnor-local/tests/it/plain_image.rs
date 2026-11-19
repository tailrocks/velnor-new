// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The plain runner image (`velnor-local/runner`, built from `local/docker/runner`) must give a
//! job the tools the production image gives it. `images/runner/ubuntu-26.04/Dockerfile` is the one
//! source of the base image and the toolchain package line; these tests fail when the local copy
//! drifts from it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_local::doctor::{PLAIN_IMAGES, RUNNER_IMAGE};

const LOCAL: &str = "../../docker/runner/Dockerfile";
const PRODUCTION: &str = "../../../images/runner/ubuntu-26.04/Dockerfile";

fn dockerfile(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
}

fn base_image(text: &str) -> &str {
    text.lines()
        .filter_map(|line| line.strip_prefix("FROM "))
        .rfind(|image| !image.starts_with("scratch"))
        .unwrap_or_else(|| unreachable!("no base image"))
}

/// The packages of the toolchain `apt-get install`: the one that names `build-essential`.
fn toolchain(text: &str) -> BTreeSet<&str> {
    let line = text
        .lines()
        .find(|line| line.contains("--no-install-recommends build-essential"))
        .unwrap_or_else(|| unreachable!("no toolchain install line"));
    let (_, packages) = line
        .split_once("--no-install-recommends ")
        .unwrap_or_else(|| unreachable!());
    packages
        .split_whitespace()
        .take_while(|word| *word != "\\" && *word != "&&")
        .collect()
}

/// The platforms recorded next to the base pin: `# platforms: linux/amd64=sha256:... linux/arm64=...`.
fn recorded_platforms(text: &str) -> BTreeMap<&str, &str> {
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("# platforms: "))
        .unwrap_or_else(|| unreachable!("no recorded platforms next to the base pin"));
    line.split_whitespace()
        .map(|entry| {
            entry
                .split_once('=')
                .unwrap_or_else(|| unreachable!("{entry} is not platform=digest"))
        })
        .collect()
}

fn digest(image: &str) -> &str {
    image
        .split_once('@')
        .unwrap_or_else(|| unreachable!("{image} has no digest"))
        .1
}

#[test]
fn the_plain_runner_image_is_built_from_the_production_base() {
    let local = dockerfile(LOCAL);
    let production = dockerfile(PRODUCTION);
    assert!(base_image(&local).starts_with("ubuntu:26.04@sha256:"));
    assert_eq!(
        base_image(&local).split('@').next(),
        base_image(&production).split('@').next()
    );
    // The production pin is the amd64 manifest of the index the plain image pins.
    assert_eq!(
        recorded_platforms(&local).get("linux/amd64").copied(),
        Some(digest(base_image(&production)))
    );
}

#[test]
fn the_plain_runner_base_is_an_index_that_covers_amd64_and_arm64() {
    let local = dockerfile(LOCAL);
    let platforms = recorded_platforms(&local);
    for platform in ["linux/amd64", "linux/arm64"] {
        assert!(platforms.contains_key(platform), "no {platform} recorded");
    }
    // A manifest of one platform is not an index: Apple Silicon would pull amd64 and fail.
    assert!(
        !platforms
            .values()
            .any(|member| *member == digest(base_image(&local))),
        "the pin is one platform's manifest, not the index"
    );
}

#[test]
fn the_plain_runner_image_installs_the_production_toolchain_packages() {
    let production = dockerfile(PRODUCTION);
    let local = dockerfile(LOCAL);
    let packages = toolchain(&production);
    assert!(packages.contains("pkg-config") && packages.contains("python3"));
    assert_eq!(toolchain(&local), packages);
}

#[test]
fn the_plain_image_the_driver_selects_is_built_from_that_dockerfile() {
    let (_, context) = PLAIN_IMAGES
        .iter()
        .find(|(image, _)| *image == RUNNER_IMAGE)
        .unwrap_or_else(|| unreachable!("no build context for {RUNNER_IMAGE}"));
    assert_eq!(*context, "local/docker/runner");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    assert_eq!(
        std::fs::read(root.join(context).join("Dockerfile")).ok(),
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(LOCAL)).ok()
    );
}
