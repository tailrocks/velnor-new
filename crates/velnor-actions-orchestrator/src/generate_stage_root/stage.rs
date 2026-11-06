// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;

use crate::OrchestratorError;
use crate::generation_filesystem::{FilesystemObjectType, GenerationRename};

use super::super::fd::Directory;
use super::guards::{
    create_private_stage, io, same_device, unsafe_path, validate_owned_directory,
    validate_private_directory, validate_target, validate_tree_owners,
};
use super::root::{FsIdentity, RootDirectory};
use super::{PRIVATE_MODE, STAGED_MODE, TARGET, cleanup};

#[derive(Debug)]
pub(in crate::generate) struct StageRoot<'root> {
    root: &'root RootDirectory,
    name: String,
    directory: Directory,
    identity: FsIdentity,
    target_identity: Option<FsIdentity>,
    target_mode: Option<u32>,
    staged_identity: Option<FsIdentity>,
    published: bool,
}

impl<'root> StageRoot<'root> {
    pub(in crate::generate) fn open(root: &'root RootDirectory) -> Result<Self, OrchestratorError> {
        root.verify_location()?;
        let (target_identity, target_mode) = validate_target(root)?;
        let mut ownership = create_private_stage(root)?;
        let identity = ownership.identity();
        let directory = match (|| {
            let directory = root
                .directory
                .open_dir(OsStr::new(ownership.name()))?;
            directory.set_mode(PRIVATE_MODE)?;
            let metadata = directory.metadata()?;
            if FsIdentity::from_metadata(&metadata) != identity {
                return Err(unsafe_path(
                    directory.path(),
                    "staging_root_identity_changed",
                ));
            }
            validate_private_directory(
                root,
                directory.as_ref(),
                &metadata,
                PRIVATE_MODE,
                "staging_metadata_not_private",
            )?;
            Ok(directory)
        })() {
            Ok(directory) => directory,
            Err(error) => return Err(ownership.abort(error)),
        };
        let stage = Self {
            root,
            name: ownership.name().to_owned(),
            identity,
            directory,
            target_identity,
            target_mode,
            staged_identity: None,
            published: false,
        };
        ownership.disarm();
        Ok(stage)
    }

    pub(in crate::generate) fn target_identity(&self) -> Option<FsIdentity> {
        self.target_identity
    }

    pub(in crate::generate) fn target_mode(&self) -> Option<u32> {
        self.target_mode
    }

    pub(in crate::generate) fn create_staged_github(&self) -> Result<Directory, OrchestratorError> {
        let name = OsStr::new(TARGET);
        self.directory.create_dir(name, STAGED_MODE)?;
        self.directory.open_dir(name)
    }

    pub(in crate::generate) fn target_directory(
        &self,
    ) -> Result<Option<Directory>, OrchestratorError> {
        let Some(expected) = self.target_identity else {
            return Ok(None);
        };
        let name = OsStr::new(TARGET);
        let directory = self.root.directory.open_dir(name)?;
        let metadata = directory.metadata()?;
        if FsIdentity::from_metadata(&metadata) != expected {
            return Err(unsafe_path(
                directory.path(),
                "target_identity_changed_before_staging",
            ));
        }
        validate_owned_directory(self.root, directory.as_ref(), &metadata)?;
        Ok(Some(directory))
    }

    pub(in crate::generate) fn verify_generated_paths(
        &self,
        paths: &[velnor_actions_workflow_renderer::guard::SafeTreePath],
    ) -> Result<(), OrchestratorError> {
        for path in paths {
            let segments: Vec<_> = path.as_str().split('/').collect();
            let mut current = self.root.directory.try_clone()?;
            for (index, segment) in segments.iter().enumerate() {
                let Some(metadata) = current.stat_opt(OsStr::new(segment))? else {
                    break;
                };
                if metadata.object_type == FilesystemObjectType::Symlink
                    && index + 1 != segments.len()
                {
                    return Err(unsafe_path(
                        &self.root.directory.path().join(path.as_str()),
                        "symlink_prefix",
                    ));
                }
                if index + 1 != segments.len() {
                    if metadata.object_type != FilesystemObjectType::Directory {
                        break;
                    }
                    let child = current.open_dir(OsStr::new(segment))?;
                    let opened = child.metadata()?;
                    if FsIdentity::from_metadata(&opened) != FsIdentity::from_metadata(&metadata) {
                        return Err(unsafe_path(
                            &self.root.directory.path().join(path.as_str()),
                            "path_identity_changed",
                        ));
                    }
                    current = child;
                }
            }
        }
        Ok(())
    }

    pub(in crate::generate) fn validate_staged(
        &mut self,
        staged: &Directory,
    ) -> Result<(), OrchestratorError> {
        self.root.verify_location()?;
        self.verify_name_identity()?;
        let staged_metadata = staged.metadata()?;
        if staged_metadata.object_type != FilesystemObjectType::Directory {
            return Err(unsafe_path(staged.path(), "staged_root_not_directory"));
        }
        if !same_device(self.identity, FsIdentity::from_metadata(&staged_metadata)) {
            return Err(unsafe_path(staged.path(), "cross_filesystem_staging"));
        }
        let staged_identity = FsIdentity::from_metadata(&staged_metadata);
        let staged_name = self.directory.stat(OsStr::new(TARGET))?;
        if FsIdentity::from_metadata(&staged_name) != staged_identity {
            return Err(unsafe_path(staged.path(), "staged_root_identity_changed"));
        }
        validate_tree_owners(self.root, staged.as_ref())?;
        let current = validate_target(self.root)?.0;
        if current != self.target_identity {
            return Err(unsafe_path(
                &self.root.directory.path().join(TARGET),
                "target_identity_changed_before_commit",
            ));
        }
        self.staged_identity = Some(staged_identity);
        Ok(())
    }

    pub(in crate::generate) fn publish_first(&mut self) -> Result<(), OrchestratorError> {
        self.verify_name_identity()?;
        self.verify_staged_name()?;
        self.root.verify_location()?;
        self.directory
            .rename_to(
                OsStr::new(TARGET),
                self.root.directory.as_ref(),
                OsStr::new(TARGET),
                GenerationRename::NoReplace,
            )
            .map_err(|error| io(&self.root.directory.path().join(TARGET), &error))?;
        self.published = true;
        Ok(())
    }

    pub(in crate::generate) fn publish_existing(&mut self) -> Result<(), OrchestratorError> {
        let expected = self
            .target_identity
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "existing_target_identity_missing".to_owned(),
            })?;
        self.verify_name_identity()?;
        self.verify_staged_name()?;
        self.root.verify_location()?;
        let current = validate_target(self.root)?.0;
        if current != Some(expected) {
            return Err(unsafe_path(
                &self.root.directory.path().join(TARGET),
                "target_identity_changed_before_commit",
            ));
        }
        self.directory
            .rename_to(
                OsStr::new(TARGET),
                self.root.directory.as_ref(),
                OsStr::new(TARGET),
                GenerationRename::Exchange,
            )
            .map_err(|error| io(&self.root.directory.path().join(TARGET), &error))?;
        self.published = true;
        Ok(())
    }

    pub(in crate::generate) fn cleanup_retired(&self) -> Result<(), OrchestratorError> {
        let expected = self
            .target_identity
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "retired_target_identity_missing".to_owned(),
            })?;
        let retired = self.directory.open_dir(OsStr::new(TARGET))?;
        let metadata = retired.metadata()?;
        if FsIdentity::from_metadata(&metadata) != expected {
            return Err(unsafe_path(retired.path(), "retired_root_identity_changed"));
        }
        validate_tree_owners(self.root, retired.as_ref())?;
        cleanup::remove_children(self.root, retired.as_ref())?;
        let current = self.directory.stat(OsStr::new(TARGET))?;
        if FsIdentity::from_metadata(&current) != expected {
            return Err(unsafe_path(retired.path(), "retired_root_identity_changed"));
        }
        self.directory.remove_dir(OsStr::new(TARGET))?;
        self.remove_stage_directory()
    }

    pub(in crate::generate) fn cleanup_after_first_publish(&self) -> Result<(), OrchestratorError> {
        self.remove_stage_directory()
    }

    pub(in crate::generate) fn abort_before_commit(
        &self,
        cause: OrchestratorError,
    ) -> OrchestratorError {
        match self.cleanup_owned_stage() {
            Ok(()) => cause,
            Err(cleanup) => OrchestratorError::io(
                self.directory.path().display().to_string(),
                format!("generation_aborted:{cause}; staging_cleanup_failed:{cleanup}"),
            ),
        }
    }

    fn verify_name_identity(&self) -> Result<(), OrchestratorError> {
        let metadata = self.root.directory.stat(OsStr::new(&self.name))?;
        if FsIdentity::from_metadata(&metadata) != self.identity {
            return Err(unsafe_path(
                &self.root.directory.path().join(&self.name),
                "staging_root_identity_changed",
            ));
        }
        let actual = self.directory.metadata()?;
        if FsIdentity::from_metadata(&actual) != self.identity {
            return Err(unsafe_path(
                self.directory.path(),
                "staging_root_identity_changed",
            ));
        }
        validate_private_directory(
            self.root,
            self.directory.as_ref(),
            &actual,
            PRIVATE_MODE,
            "staging_metadata_not_private",
        )?;
        Ok(())
    }

    fn verify_staged_name(&self) -> Result<(), OrchestratorError> {
        let expected = self
            .staged_identity
            .ok_or_else(|| OrchestratorError::Contract {
                problem: "staged_root_not_validated".to_owned(),
            })?;
        let metadata = self.directory.stat(OsStr::new(TARGET))?;
        if FsIdentity::from_metadata(&metadata) != expected
            || metadata.object_type != FilesystemObjectType::Directory
        {
            return Err(unsafe_path(
                &self.directory.path().join(TARGET),
                "staged_root_identity_changed",
            ));
        }
        Ok(())
    }

    fn remove_stage_directory(&self) -> Result<(), OrchestratorError> {
        self.verify_name_identity()?;
        cleanup::remove_tree(
            self.root,
            self.root.directory.as_ref(),
            OsStr::new(&self.name),
            self.identity,
        )
    }

    fn cleanup_owned_stage(&self) -> Result<(), OrchestratorError> {
        self.verify_name_identity()?;
        let entries = self.directory.entries()?;
        for name in entries {
            let metadata = self.directory.stat(&name)?;
            if metadata.object_type == FilesystemObjectType::Directory {
                let child = self.directory.open_dir(&name)?;
                let identity = FsIdentity::from_metadata(&metadata);
                let opened = child.metadata()?;
                if FsIdentity::from_metadata(&opened) != identity {
                    return Err(unsafe_path(
                        &self.directory.path().join(&name),
                        "cleanup_entry_identity_changed",
                    ));
                }
                validate_tree_owners(self.root, child.as_ref())?;
                cleanup::remove_children(self.root, child.as_ref())?;
                let current = self.directory.stat(&name)?;
                if FsIdentity::from_metadata(&current) != identity {
                    return Err(unsafe_path(
                        &self.directory.path().join(&name),
                        "cleanup_entry_identity_changed",
                    ));
                }
                self.directory.remove_dir(&name)?;
            } else {
                self.directory.remove_file(&name)?;
            }
        }
        self.remove_stage_directory()
    }
}

impl Drop for StageRoot<'_> {
    fn drop(&mut self) {
        if !self.published {
            let _cleanup = self.cleanup_owned_stage();
        }
    }
}
