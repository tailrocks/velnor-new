// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::super::ir::Job;
use super::{LaneClass, Placement};
use crate::config::BUN_JOB_PREFIX;
use crate::errors::ContractError;

#[derive(Debug, Clone)]
pub struct BunLanePlan {
    routes: BTreeMap<String, (LaneClass, Placement)>,
    id_map: BTreeMap<String, Vec<String>>,
    scale_token: Option<String>,
}

impl BunLanePlan {
    pub(super) fn from_placements(
        placements: &BTreeMap<String, (LaneClass, Placement)>,
        id_map: &BTreeMap<String, Vec<String>>,
        scale_token: Option<String>,
    ) -> Result<Self, ContractError> {
        let mut routes = BTreeMap::new();
        for (logical_id, (class, placement)) in placements {
            if logical_id.starts_with(BUN_JOB_PREFIX) && *placement == Placement::Both {
                return Err(plan_error("paired_bun_placement", logical_id));
            }
            routes.insert(logical_id.clone(), (*class, *placement));
        }
        Ok(Self {
            routes,
            id_map: id_map.clone(),
            scale_token,
        })
    }

    #[must_use]
    pub fn logical_id_for_output(&self, output_id: &str) -> Option<&str> {
        self.routes
            .get_key_value(output_id)
            .map(|(id, _)| id.as_str())
    }

    pub fn output_ids(&self, logical_id: &str) -> Result<Vec<String>, ContractError> {
        self.route(logical_id)?;
        Ok(vec![logical_id.to_owned()])
    }

    pub fn project(
        &self,
        logical_id: &str,
        canonical_job: &Job,
    ) -> Result<Vec<(String, Job)>, ContractError> {
        let (class, placement) = self.route(logical_id)?;
        let projected = super::emit_job_variants(
            logical_id,
            canonical_job,
            self.scale_token.as_deref(),
            *class,
            *placement,
            &self.id_map,
        )?;
        if projected.len() != 1 || projected[0].0 != logical_id {
            return Err(plan_error("bun_output_id_mismatch", logical_id));
        }
        Ok(projected)
    }

    fn route(&self, logical_id: &str) -> Result<&(LaneClass, Placement), ContractError> {
        self.routes
            .get(logical_id)
            .filter(|_| logical_id.starts_with(BUN_JOB_PREFIX))
            .ok_or_else(|| plan_error("unknown_logical_id", logical_id))
    }
}

fn plan_error(problem: &str, logical_id: &str) -> ContractError {
    ContractError::identity("bun_lane_plan", format!("{problem}:{logical_id}"))
}
