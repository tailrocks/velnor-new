// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use velnor_model::ReleaseAsset;

use crate::error::CiError;

use super::release_assets::hex;

const MAX_ASSET_BYTES: u64 = 4 << 30;

/// One `--asset` argument, checked before anything runs.
pub(super) struct Entry(ReleaseAsset);

/// The folder an asset is stored in, resolved under the workspace without following a link.
pub(super) struct Placement {
    file: PathBuf,
    folder: PathBuf,
}

impl Entry {
    pub(super) fn parse(text: &str) -> Result<Self, CiError> {
        ReleaseAsset::from_argument(text).map(Self).map_err(|e| {
            CiError::new(format!(
                "--asset: invalid {} {:?}",
                e.field, e.value
            ))
        })
    }

    pub(super) fn name(&self) -> &str {
        &self.0.name
    }

    pub(super) fn describe(&self) -> String {
        let asset = &self.0;
        format!("{}/{} {} {}", asset.owner, asset.repo, asset.tag, asset.name)
    }

    pub(super) fn release_path(&self) -> String {
        let asset = &self.0;
        format!("/repos/{}/{}/releases/tags/{}", asset.owner, asset.repo, asset.tag)
    }

    pub(super) fn asset_path(&self, id: u64) -> String {
        format!("/repos/{}/{}/releases/assets/{id}", self.0.owner, self.0.repo)
    }

    pub(super) fn expect_digest(&self, actual: &str) -> Result<(), CiError> {
        if actual == self.0.sha256 {
            Ok(())
        } else {
            Err(CiError::new(format!(
                "{} has sha256 {actual}, and the configuration requires {}",
                self.describe(),
                self.0.sha256
            )))
        }
    }

    /// Walk `dest` from the workspace root one name at a time; a link or a file in the way is
    /// an error, and `create` makes the missing folders.
    pub(super) fn place(&self, root: &Path, create: bool) -> Result<Placement, CiError> {
        let mut folder = root.to_path_buf();
        for part in self.0.dest.split('/') {
            folder.push(part);
            match fs::symlink_metadata(&folder) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    return Err(CiError::new(format!(
                        "{} is a link or a file, not a folder; the destination is refused",
                        folder.display()
                    )));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                    fs::create_dir(&folder)
                        .map_err(|e| CiError::new(format!("cannot create {}: {e}", folder.display())))?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(CiError::new(format!("{}: {e}", folder.display()))),
            }
        }
        Ok(Placement {
            file: folder.join(&self.0.name),
            folder,
        })
    }
}

impl Placement {
    pub(super) fn file(&self) -> &Path {
        &self.file
    }

    /// Stream `reader` into a new file beside the target, check its digest with `check`, and
    /// only then move it into place; any failure removes the partial file.
    pub(super) fn store(
        &self,
        mut reader: impl Read,
        check: impl FnOnce(&str) -> Result<(), CiError>,
    ) -> Result<(), CiError> {
        let partial = self
            .folder
            .join(format!(".velnor-partial-{}", std::process::id()));
        let result = self.write(&partial, &mut reader, check);
        if result.is_err() {
            drop(fs::remove_file(&partial));
        }
        result
    }

    fn write(
        &self,
        partial: &Path,
        reader: &mut impl Read,
        check: impl FnOnce(&str) -> Result<(), CiError>,
    ) -> Result<(), CiError> {
        let fail = |what: &str, e: std::io::Error| CiError::new(format!("{what}: {e}"));
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(partial)
            .map_err(|e| fail(&format!("cannot create {}", partial.display()), e))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 1 << 16];
        let mut total = 0_u64;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|e| fail("the download failed", e))?;
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > MAX_ASSET_BYTES {
                return Err(CiError::new(format!(
                    "the asset is larger than {MAX_ASSET_BYTES} bytes"
                )));
            }
            hasher.update(&buffer[..read]);
            out.write_all(&buffer[..read])
                .map_err(|e| fail("cannot write the asset", e))?;
        }
        out.sync_all().map_err(|e| fail("cannot write the asset", e))?;
        drop(out);
        check(&hex(&hasher.finalize()))?;
        fs::rename(partial, &self.file)
            .map_err(|e| fail(&format!("cannot move the asset to {}", self.file.display()), e))
    }
}
