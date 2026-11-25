// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{ImageConfig, MisePlatform, RepoFacts, RunnerLabel};

use crate::ImageError;

fn linux_x64(label: &RunnerLabel) -> Result<(), ImageError> {
    match MisePlatform::runner(label.as_str()) {
        Some(platform) if platform.lock_key() != "linux-x64" => Err(ImageError::NotLinuxRunner {
            runner: label.to_string(),
        }),
        _ => Ok(()),
    }
}

pub(crate) fn check_runners(facts: &RepoFacts, images: &[ImageConfig]) -> Result<(), ImageError> {
    let runner = &facts.config.runner;
    let mut control: Option<(&str, Option<&RunnerLabel>)> = None;
    for image in images {
        for label in [&image.control_runner, &image.publish_runner]
            .into_iter()
            .flatten()
        {
            linux_x64(label)?;
        }
        if !facts.config.hosted_runners.contains(runner) && image.publish_runner.is_none() {
            return Err(ImageError::PublishPoolShared {
                image: image.name.clone(),
                runner: runner.to_string(),
            });
        }
        let chosen = image.control_runner.as_ref();
        match control {
            None => control = Some((&image.name, chosen)),
            Some((first, other)) if !same(other, chosen) => {
                return Err(ImageError::ControlRunnerDiffers {
                    first: first.to_owned(),
                    second: image.name.clone(),
                });
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn same(left: Option<&RunnerLabel>, right: Option<&RunnerLabel>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.as_str().eq_ignore_ascii_case(right.as_str()),
        _ => false,
    }
}
