// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::RustStackConfig;
use velnor_actions_contract::workflow::cache_family::{S256, SliceId};

use crate::OrchestratorError;
use crate::crate_jobs::is_rust;
use crate::discover::Discovery;

const CONFIG_KEY: &str = "stacks.rust.state_slices";

#[derive(Debug, Clone, Default)]
pub(crate) struct SliceAssignment {
    by_package: BTreeMap<String, SliceId>,
}

impl SliceAssignment {
    pub(crate) fn from_config(
        rust: Option<&RustStackConfig>,
        discovery: &Discovery,
    ) -> Result<Self, OrchestratorError> {
        let mut by_package = BTreeMap::new();
        for (slice, packages) in rust.iter().flat_map(|rust| &rust.state_slices) {
            let id = SliceId::parse(slice)?;
            for package in packages {
                if !discovery
                    .proposals
                    .iter()
                    .any(|task| task.display_name == *package && is_rust(task))
                {
                    return Err(OrchestratorError::config(
                        crate::config::CONFIG_REL,
                        format!("{CONFIG_KEY}.{slice}"),
                        format!("unknown_rust_package:{package}"),
                    ));
                }
                by_package.insert(package.clone(), id.clone());
            }
        }
        Ok(Self { by_package })
    }

    pub(crate) fn slice_for(&self, job_id: &str, package: &str) -> Result<SliceId, OrchestratorError> {
        match self.by_package.get(package) {
            Some(slice) => Ok(slice.clone()),
            None => Ok(SliceId::parse(job_id)?),
        }
    }

    pub(crate) fn check_disjoint(&self, job_ids: &BTreeSet<&str>) -> Result<(), OrchestratorError> {
        for slice in self.by_package.values() {
            if job_ids.contains(slice.as_str()) {
                return Err(OrchestratorError::config(
                    crate::config::CONFIG_REL,
                    format!("{CONFIG_KEY}.{slice}"),
                    "state_slice_names_a_crate_job",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SliceBuild {
    pub(crate) recipes: BTreeMap<String, S256>,
    pub(crate) packages: BTreeMap<SliceId, Vec<String>>,
    pub(crate) assignment: BTreeMap<String, SliceId>,
}

impl SliceBuild {
    pub(crate) fn assign(&mut self, job_id: &str, slice: &SliceId, package_id: &str) {
        self.assignment.insert(job_id.to_owned(), slice.clone());
        self.packages
            .entry(slice.clone())
            .or_default()
            .push(package_id.to_owned());
    }
}
