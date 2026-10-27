// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use tempfile::TempDir;

pub(crate) mod oci;
mod repository;

use repository::GitFixture;

pub(crate) struct AdmissionFixture {
    temp_directory: TempDir,
    pub(crate) root: PathBuf,
    pub(crate) image_dir: PathBuf,
    pub(crate) source: String,
    pub(crate) output: PathBuf,
    pub(crate) images: ImageSet,
    repository: GitFixture,
}

#[derive(Clone)]
pub(crate) struct ImageSet {
    pub(crate) runner: oci::OciImage,
    pub(crate) dind: oci::OciImage,
    pub(crate) resource_probe: oci::OciImage,
}

impl AdmissionFixture {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let repository = GitFixture::new(base.join("checkout"))?;
        let root = repository.root().to_path_buf();
        let source = repository.head()?;
        let image_dir = base.join("inputs");
        fs::create_dir_all(&image_dir)?;
        let images = oci::admission_images(&image_dir, &source)?;
        let output = base.join("descriptors.json");
        Ok(Self {
            temp_directory: temp,
            root,
            image_dir,
            source,
            output,
            images,
            repository,
        })
    }

    pub(crate) fn mutate_checkout(&self, mutation: &str) -> Result<(), Box<dyn Error>> {
        self.repository.mutate_checkout(mutation)
    }

    pub(crate) fn hide_and_change_lock(&self, flag: &str) -> Result<(), Box<dyn Error>> {
        self.repository.hide_and_change_lock(flag)
    }

    pub(crate) fn remove_source_lock_and_create_ignored_replacement(
        &mut self,
    ) -> Result<(), Box<dyn Error>> {
        self.repository
            .remove_source_lock_and_create_ignored_replacement()?;
        self.source = self.repository.head()?;
        self.images = oci::admission_images(&self.image_dir, &self.source)?;
        Ok(())
    }

    pub(crate) fn advance_head(&self) -> Result<(), Box<dyn Error>> {
        self.repository.advance_head()
    }

    pub(crate) fn commit_lock(&mut self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        self.repository.commit_lock(bytes)?;
        self.source = self.repository.head()?;
        self.images = oci::admission_images(&self.image_dir, &self.source)?;
        Ok(())
    }
}

impl ImageSet {
    pub(crate) fn entries(&self) -> [(&'static str, &oci::OciImage, &'static str); 3] {
        [
            ("runner", &self.runner, "ghcr.io/tailrocks/velnor-runner"),
            ("dind", &self.dind, "ghcr.io/tailrocks/velnor-dind"),
            (
                "resource_probe",
                &self.resource_probe,
                "ghcr.io/tailrocks/velnor-resource-probe",
            ),
        ]
    }

    pub(crate) fn replace(&mut self, index: usize, image: oci::OciImage) {
        match index {
            0 => self.runner = image,
            1 => self.dind = image,
            2 => self.resource_probe = image,
            _ => unreachable!("only the three admitted image slots exist"),
        }
    }

    pub(crate) fn bytes(&self) -> Result<[Vec<u8>; 3], std::io::Error> {
        Ok([
            fs::read(&self.runner.path)?,
            fs::read(&self.dind.path)?,
            fs::read(&self.resource_probe.path)?,
        ])
    }
}

pub(crate) fn invoke(
    fixture: &AdmissionFixture,
    images: &ImageSet,
) -> Result<Output, std::io::Error> {
    invoke_to(fixture, images, &fixture.source, &fixture.output)
}

pub(crate) fn invoke_with_source(
    fixture: &AdmissionFixture,
    images: &ImageSet,
    source: &str,
) -> Result<Output, std::io::Error> {
    invoke_to(fixture, images, source, &fixture.output)
}

pub(crate) fn invoke_to(
    fixture: &AdmissionFixture,
    images: &ImageSet,
    source: &str,
    output: &Path,
) -> Result<Output, std::io::Error> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command
        .env_clear()
        .env("PATH", path)
        .env("HOME", fixture.temp_directory.path().join("home"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GITHUB_SHA", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .env(
            "GITHUB_WORKFLOW_SHA",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .args(["image-maintenance", "admit-runner-images", "--root"])
        .arg(&fixture.root)
        .args(["--source-commit", source, "--runner-archive"])
        .arg(&images.runner.path)
        .arg("--dind-archive")
        .arg(&images.dind.path)
        .arg("--probe-archive")
        .arg(&images.resource_probe.path)
        .arg("--descriptors-output")
        .arg(output)
        .output()
}

pub(crate) fn lock_with_mutation(pin: &str, mutation: &str) -> Vec<u8> {
    repository::lock_with_mutation(pin, mutation)
}
