// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The plain runner image (`velnor-local/runner`, built from `local/docker/runner`) must give a
//! job the tools the production image gives it. `images/runner/ubuntu-26.04/Dockerfile` is the one
//! source of the base image and the toolchain package line; these tests fail when the local copy
//! drifts from it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_local::doctor::{CACHE_IMAGE, PLAIN_IMAGES, RUNNER_IMAGE};

const LOCAL: &str = "../../local/docker/runner/Dockerfile";
const PRODUCTION: &str = "../../images/runner/ubuntu-26.04/Dockerfile";
const CACHE_DOCKERFILE: &str = "../../local/docker/velnor-local/Dockerfile";

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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(
        std::fs::read(root.join(context).join("Dockerfile")).ok(),
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(LOCAL)).ok()
    );
}

/// What the image gives a job's environment: every `ENV` key, and every instruction that names
/// `mise` or sets `PATH`. A job inherits these from the image, so a difference between the plain
/// and the production image shows up as a job that passes in one and fails in the other.
fn job_environment(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .flat_map(|line| {
            let env = line.strip_prefix("ENV ").map(|rest| {
                let key = rest.split(['=', ' ']).next().unwrap_or_default();
                format!("ENV {key}")
            });
            let activation = (line.contains("mise") || line.contains("PATH="))
                .then(|| line.trim_end_matches(['\\', ' ']).to_owned());
            env.into_iter().chain(activation)
        })
        .collect()
}

fn user_command(text: &str) -> &str {
    let at = text
        .find("useradd")
        .unwrap_or_else(|| unreachable!("no useradd"));
    text[at..].lines().next().unwrap_or_default()
}

#[test]
fn the_plain_runner_image_gives_jobs_the_environment_the_production_image_gives() {
    let production = job_environment(&dockerfile(PRODUCTION));
    let local = job_environment(&dockerfile(LOCAL));
    assert_eq!(
        local, production,
        "plain and production disagree on the PATH, mise activation and ENV a job inherits"
    );
}

#[test]
fn the_plain_runner_user_is_the_production_user() {
    let (local, production) = (dockerfile(LOCAL), dockerfile(PRODUCTION));
    for flag in ["--uid 1000", "--gid 1000", "--shell /bin/bash"] {
        assert!(user_command(&production).contains(flag), "{flag}");
        assert!(user_command(&local).contains(flag), "plain image: {flag}");
    }
}

#[test]
fn the_cache_server_image_is_built_from_the_repository_root() {
    let (_, build) = PLAIN_IMAGES
        .iter()
        .find(|(image, _)| *image == CACHE_IMAGE)
        .unwrap_or_else(|| unreachable!("no build command for {CACHE_IMAGE}"));
    let words: Vec<&str> = build.split_whitespace().collect();
    assert_eq!(
        words.as_slice(),
        ["-f", "local/docker/velnor-local/Dockerfile", "."]
    );
    let context = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let context = context.join(words[2]);
    for line in dockerfile(CACHE_DOCKERFILE).lines() {
        let Some(copy) = line.strip_prefix("COPY ") else {
            continue;
        };
        if copy.starts_with("--from") {
            continue;
        }
        let sources = copy.split_whitespace().collect::<Vec<_>>();
        for source in &sources[..sources.len() - 1] {
            assert!(
                context.join(source).exists(),
                "{source} is not under the build context {}",
                context.display()
            );
        }
    }
}
