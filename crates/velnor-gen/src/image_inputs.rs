// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{ImageConfig, RepoPath, RepoView};

use crate::ImageError;

pub(crate) fn check(view: &RepoView, image: &ImageConfig) -> Result<(), ImageError> {
    for input in &image.inputs {
        let Ok(path) = RepoPath::new(input.as_str()) else {
            return Err(untracked(image, input));
        };
        let directory = format!("{input}/");
        let tracked = view.tracked_paths.contains(&path)
            || view
                .tracked_paths
                .iter()
                .any(|tracked| tracked.as_str().starts_with(&directory));
        if !tracked {
            return Err(untracked(image, input));
        }
        let linked = std::iter::successors(Some(path), RepoPath::parent)
            .any(|part| view.symlinks.contains(&part) || view.gitlinks.contains(&part));
        if linked {
            return Err(ImageError::InputLinked {
                image: image.name.clone(),
                path: input.clone(),
            });
        }
    }
    Ok(())
}

fn untracked(image: &ImageConfig, input: &str) -> ImageError {
    ImageError::InputUntracked {
        image: image.name.clone(),
        path: input.to_owned(),
    }
}
