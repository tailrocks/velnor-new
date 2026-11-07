use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use crate::gc1_scene::fail;
use crate::gc1_seal_units::{group_is_live, process_group};

pub(super) struct LaunchGuardianCleanup {
    directory: PathBuf,
    selector_group: u32,
    groups: BTreeSet<u32>,
}

impl LaunchGuardianCleanup {
    pub(super) fn new(directory: PathBuf, selector_group: u32) -> Self {
        Self {
            directory,
            selector_group,
            groups: BTreeSet::from([selector_group]),
        }
    }

    pub(super) fn capture(&mut self) -> Result<(), Box<dyn Error>> {
        for (name, fields) in [
            ("group-log", 1),
            ("launch-inventory", 1),
            ("launch-release.started", 1),
        ] {
            let path = self.directory.join(name);
            let contents = match fs::read_to_string(path) {
                Ok(contents) => contents,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            for line in contents.lines() {
                let field = line
                    .split_whitespace()
                    .nth(fields - 1)
                    .ok_or_else(|| fail("launch custody record was incomplete"))?;
                let pid_or_group: u32 = field.parse()?;
                if pid_or_group == 0 {
                    return Err(fail("launch custody record contained a zero process id"));
                }
                let group = if name == "launch-inventory" || name == "launch-release.started" {
                    process_group(pid_or_group).unwrap_or(pid_or_group)
                } else {
                    pid_or_group
                };
                self.groups.insert(group);
            }
        }
        Ok(())
    }

    pub(super) fn active_guardian_groups(&self) -> usize {
        self.groups
            .iter()
            .filter(|group| **group != self.selector_group && group_is_live(**group))
            .count()
    }

    pub(super) fn contains_group(&self, group: u32) -> bool {
        self.groups.contains(&group)
    }

    pub(super) fn cleanup(&mut self) -> Result<(), Box<dyn Error>> {
        let mut errors = Vec::new();
        if let Err(error) = self.capture() {
            errors.push(error.to_string());
        }
        for group in self.groups.iter().copied() {
            if let Err(error) = super::super::call_helpers::terminate_group_bounded(group) {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(fail(&errors.join("\n")))
        }
    }

    pub(super) fn surviving_groups(&self) -> Vec<u32> {
        self.groups
            .iter()
            .copied()
            .filter(|group| group_is_live(*group))
            .collect()
    }
}

impl Drop for LaunchGuardianCleanup {
    fn drop(&mut self) {
        drop(self.cleanup());
    }
}
