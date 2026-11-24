// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::image::ImageBuild;
use crate::image_build::{job_id, staged};

pub(crate) struct Compile<'a> {
    pub(crate) members: Vec<&'a ImageBuild<'a>>,
}

impl Compile<'_> {
    pub(crate) fn id(&self) -> String {
        job_id(self.members[0].image, "build")
    }

    pub(crate) fn bins(&self) -> Vec<&str> {
        let mut bins: Vec<&str> = Vec::new();
        for member in &self.members {
            for bin in &member.image.bins {
                if !bins.contains(&bin.as_str()) {
                    bins.push(bin);
                }
            }
        }
        bins
    }

    pub(crate) fn packages(&self) -> Vec<&str> {
        let mut packages: Vec<&str> = Vec::new();
        for member in &self.members {
            for package in &member.packages {
                if !packages.contains(package) {
                    packages.push(package);
                }
            }
        }
        packages
    }
}

fn features<'a>(build: &'a ImageBuild<'_>) -> Vec<&'a str> {
    let mut features: Vec<&str> = build.image.features.iter().map(String::as_str).collect();
    features.sort_unstable();
    features
}

fn same_compilation(left: &ImageBuild<'_>, right: &ImageBuild<'_>) -> bool {
    let (left_image, right_image) = (left.image, right.image);
    left.root == right.root
        && left_image.profile == right_image.profile
        && left_image.glibc == right_image.glibc
        && left_image.openssl == right_image.openssl
        && left_image.default_features == right_image.default_features
        && features(left) == features(right)
}

fn overlap(left: &str, right: &str) -> bool {
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

fn joins(group: &Compile<'_>, build: &ImageBuild<'_>) -> bool {
    same_compilation(group.members[0], build)
        && group
            .members
            .iter()
            .all(|member| !overlap(&staged(member.image), &staged(build.image)))
}

pub(crate) fn compile_groups<'a>(builds: &'a [ImageBuild<'a>]) -> Vec<Compile<'a>> {
    let mut groups: Vec<Compile<'a>> = Vec::new();
    for build in builds {
        match groups.iter_mut().find(|group| joins(group, build)) {
            Some(group) => group.members.push(build),
            None => groups.push(Compile {
                members: vec![build],
            }),
        }
    }
    groups
}
