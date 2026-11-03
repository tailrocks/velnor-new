// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::gc1_scene::{Scene, fail};

pub(crate) struct Key {
    pub(crate) snapshot_prefix: String,
    pub(crate) restore_prefix: String,
}

impl Key {
    pub(crate) fn exact(&self) -> String {
        format!("{}{PAYLOAD_SEGMENT}", self.snapshot_prefix)
    }
}

const PAYLOAD_SEGMENT: &str =
    "s256-2222222222222222222222222222222222222222222222222222222222222222";

pub(crate) const DEFAULT_SNAPSHOT: &str =
    "s256-1111111111111111111111111111111111111111111111111111111111111111";

impl Scene {
    pub(crate) fn output_value(&self, key: &str) -> Result<String, Box<dyn Error>> {
        let text = std::fs::read_to_string(&self.github_output)?;
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .map(str::to_owned)
            .ok_or_else(|| fail(&format!("github output lost: {key}")))
    }

    pub(crate) fn output_block(&self, key: &str) -> Result<Vec<String>, Box<dyn Error>> {
        let text = std::fs::read_to_string(&self.github_output)?;
        let opener = format!("{key}<<");
        let mut lines = text.lines();
        let delimiter = lines
            .by_ref()
            .find_map(|line| line.strip_prefix(&opener))
            .ok_or_else(|| fail(&format!("github output block lost: {key}")))?;
        Ok(lines
            .take_while(|line| *line != delimiter)
            .map(str::to_owned)
            .collect())
    }

    pub(crate) fn discover(&self) -> Result<Key, Box<dyn Error>> {
        self.discover_with(&[])
    }

    pub(crate) fn discover_with(&self, env: &[(&str, &str)]) -> Result<Key, Box<dyn Error>> {
        self.discover_unset(env, &[])
    }

    pub(crate) fn discover_script(&self, script: &str) -> Result<Key, Box<dyn Error>> {
        if let Err(error) = std::fs::remove_file(&self.github_output)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(error.into());
        }
        self.run_script_unset(script, &[], &[], None)?;
        self.key_from_outputs()
    }

    pub(crate) fn discover_outputs(
        &self,
        env: &[(&str, &str)],
        unset: &[&str],
    ) -> Result<String, Box<dyn Error>> {
        if let Err(error) = std::fs::remove_file(&self.github_output)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            return Err(error.into());
        }
        self.run_script_unset(&self.scripts.discover, env, unset, None)?;
        Ok(std::fs::read_to_string(&self.github_output)?)
    }

    pub(crate) fn discover_unset(
        &self,
        env: &[(&str, &str)],
        unset: &[&str],
    ) -> Result<Key, Box<dyn Error>> {
        self.discover_outputs(env, unset)?;
        if self.output_value("provenance")? != "trusted" {
            return Err(fail("discover misclassified a trusted push"));
        }
        self.key_from_outputs()
    }

    fn record_value(&self, name: &str) -> Result<String, Box<dyn Error>> {
        let text = std::fs::read_to_string(self.runner.join("velnor/state-discovery.record"))?;
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")))
            .map(str::to_owned)
            .ok_or_else(|| fail(&format!("discovery record lost: {name}")))
    }

    pub(crate) fn key_from_outputs(&self) -> Result<Key, Box<dyn Error>> {
        let restore_prefix = self.record_value("restore_prefix")?;
        let snapshot_prefix = self.record_value("snapshot_prefix")?;
        if !snapshot_prefix.starts_with(&restore_prefix) {
            return Err(fail(
                "the snapshot prefix does not extend the compat prefix",
            ));
        }
        if self.output_value("key")? != snapshot_prefix {
            return Err(fail("the lookup key is not the snapshot prefix"));
        }
        Ok(Key {
            snapshot_prefix,
            restore_prefix,
        })
    }
}
