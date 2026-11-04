// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::{Path, PathBuf};

use crate::gc1_scene::{Scene, Tree, fail};
use crate::gc1_scene_tree::expect_denied;

impl Scene {
    pub(crate) fn gone(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_err()
    }

    pub(crate) fn assert_clean_exit(&self) -> Result<(), Box<dyn Error>> {
        for (path, what) in [
            (self.stage(), "stage"),
            (self.state_root(), "state root"),
            (self.scratch_dir(), "recover scratch"),
        ] {
            if !Self::gone(&path) {
                return Err(fail(&format!("recover left the {what}")));
            }
        }
        Ok(())
    }

    pub(crate) fn assert_cold(&self, output: &str, token: &str) -> Result<(), Box<dyn Error>> {
        for needle in ["::warning::cache_corrupt", token] {
            if !output.contains(needle) {
                return Err(fail(&format!("recover output lost {needle}: {output}")));
            }
        }
        for (path, what) in [
            (self.recovery_record(), "recovery record"),
            (self.payload(), "payload"),
        ] {
            if !Self::gone(&path) {
                return Err(fail(&format!("refused archive left the {what}")));
            }
        }
        if !self.store_files()?.is_empty() {
            return Err(fail("refused archive reached the store"));
        }
        self.assert_clean_exit()
    }

    pub(crate) fn assert_silent_cold(
        &self,
        output: &str,
        token: &str,
    ) -> Result<(), Box<dyn Error>> {
        if output.contains("::warning::") || !output.contains(token) {
            return Err(fail(&format!("recover must go cold silently: {output}")));
        }
        if !Self::gone(&self.recovery_record()) || !self.store_files()?.is_empty() {
            return Err(fail("silent cold exit imported something"));
        }
        self.assert_clean_exit()
    }

    pub(crate) fn probe_modes(&self) -> Result<(), Box<dyn Error>> {
        expect_denied(&self.scratch)
    }

    fn confined(
        &self,
        before: &Tree,
        recovered: bool,
        also_consumed: &[PathBuf],
    ) -> Result<(), Box<dyn Error>> {
        let after = self.tree()?;
        let mut consumed = vec![self.payload(), self.state_root(), self.stage()];
        consumed.extend_from_slice(also_consumed);
        let mut produced = vec![self.github_output.clone()];
        produced.extend_from_slice(also_consumed);
        if recovered {
            produced.extend([
                self.recovery_record(),
                self.store(),
                self.workspace.join("target"),
            ]);
        }
        for (path, entry) in &after {
            if before.get(path) != Some(entry)
                && !produced.iter().any(|root| path.starts_with(root))
            {
                return Err(fail(&format!("recover wrote {}", path.display())));
            }
        }
        for path in before.keys() {
            if !after.contains_key(path) && !consumed.iter().any(|root| path.starts_with(root)) {
                return Err(fail(&format!("recover removed {}", path.display())));
            }
        }
        Ok(())
    }

    pub(crate) fn assert_confined(&self, before: &Tree) -> Result<(), Box<dyn Error>> {
        self.confined(before, false, &[])
    }

    pub(crate) fn assert_confined_beyond(
        &self,
        before: &Tree,
        also_consumed: &[PathBuf],
    ) -> Result<(), Box<dyn Error>> {
        self.confined(before, false, also_consumed)
    }

    pub(crate) fn assert_confined_recovered(&self, before: &Tree) -> Result<(), Box<dyn Error>> {
        self.confined(before, true, &[])
    }
}
