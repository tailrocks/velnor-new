//! Static Playwright leg matrices of sharded Bun suite jobs.
//!
//! A sharded suite job carries the canonical leg list as the
//! [`BUN_LEGS_ENV`] marker on its script step (and the optional cap as
//! [`BUN_MAX_PARALLEL_ENV`]). Emission parses and re-validates the marker,
//! scrubs it, and inserts `strategy` (`fail-fast: false`, optional
//! `max-parallel`, `matrix.include` of the legs) before the job's `steps`.
//! The include form keeps `(project, shard, total)` together: a cross product
//! would also cross `total`.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{Job, StepKind};

use super::{insert_job_key, matrix_invalid};
use crate::{RenderError, yaml::Yaml};

/// Leg marker: `leg/project/shard/total` entries joined by `,` (empty project
/// when the suite shards over every default project).
pub(crate) const BUN_LEGS_ENV: &str = "VELNOR_BUN_LEGS";
/// Leg marker: optional `strategy.max-parallel` cap (`1..=legs`).
pub(crate) const BUN_MAX_PARALLEL_ENV: &str = "VELNOR_BUN_MAX_PARALLEL";

/// One matrix leg of a sharded Playwright suite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunLeg {
    /// 1-based ordinal, unique within the job.
    pub leg: u16,
    /// Playwright project this leg runs, when the suite declares projects.
    pub project: Option<String>,
    /// 1-based shard within the project (or the whole run).
    pub shard: u16,
    /// Shards per project (or of the whole run).
    pub total: u16,
}

/// Every leg of one sharded suite job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunLegMatrix {
    /// Legs sorted by `(project, shard)`, ordinals `1..=legs.len()`.
    pub legs: Vec<BunLeg>,
    /// Optional `strategy.max-parallel`.
    pub max_parallel: Option<u16>,
}

impl BunLegMatrix {
    /// Every leg of `projects` (empty: one unnamed group) times `shards`.
    #[must_use]
    pub fn new(projects: &[String], shards: u16, max_parallel: Option<u16>) -> Self {
        let groups: Vec<Option<String>> = if projects.is_empty() {
            vec![None]
        } else {
            projects.iter().cloned().map(Some).collect()
        };
        let mut legs = Vec::new();
        let mut leg = 0_u16;
        for project in groups {
            for shard in 1..=shards {
                leg = leg.saturating_add(1);
                legs.push(BunLeg {
                    leg,
                    project: project.clone(),
                    shard,
                    total: shards,
                });
            }
        }
        Self { legs, max_parallel }
    }

    /// True when legs pass `--project` (the suite declares projects).
    #[must_use]
    pub fn has_projects(&self) -> bool {
        self.legs.iter().any(|leg| leg.project.is_some())
    }

    /// True when legs pass `--shard` (more than one shard per group).
    #[must_use]
    pub fn is_sharded(&self) -> bool {
        self.legs.iter().any(|leg| leg.total > 1)
    }

    /// Canonical marker value.
    #[must_use]
    pub(crate) fn marker(&self) -> String {
        self.legs
            .iter()
            .map(|leg| {
                format!(
                    "{}/{}/{}/{}",
                    leg.leg,
                    leg.project.as_deref().unwrap_or_default(),
                    leg.shard,
                    leg.total
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Check the leg list is the canonical one for its own projects and shards.
    ///
    /// # Errors
    ///
    /// `bun_matrix_*` for an empty, oversized, unordered or inconsistent list.
    pub fn validate(&self) -> Result<(), RenderError> {
        let max = usize::from(velnor_actions_contract::MAX_SUITE_PARTITIONS);
        if self.legs.is_empty() || self.legs.len() > max {
            return Err(matrix_invalid("bun_matrix_bad_leg_count"));
        }
        let total = self.legs[0].total;
        let mut projects = Vec::new();
        for leg in &self.legs {
            if leg
                .project
                .as_deref()
                .is_some_and(|name| !velnor_actions_contract::config::is_valid_project_name(name))
            {
                return Err(matrix_invalid("bun_matrix_bad_project"));
            }
            if projects.last() != Some(&leg.project) {
                projects.push(leg.project.clone());
            }
        }
        let named: Vec<String> = projects.iter().flatten().cloned().collect();
        if named.len() != projects.len() && projects != [None] {
            return Err(matrix_invalid("bun_matrix_mixed_projects"));
        }
        if named.windows(2).any(|pair| pair[0] >= pair[1])
            || named.iter().collect::<BTreeSet<_>>().len() != named.len()
        {
            return Err(matrix_invalid("bun_matrix_projects_unsorted"));
        }
        if total == 0 || *self != Self::new(&named, total, self.max_parallel) {
            return Err(matrix_invalid("bun_matrix_not_canonical"));
        }
        if self
            .max_parallel
            .is_some_and(|cap| cap == 0 || usize::from(cap) > self.legs.len())
        {
            return Err(matrix_invalid("bun_matrix_bad_max_parallel"));
        }
        Ok(())
    }

    fn parse(legs: &str, max_parallel: Option<&str>) -> Result<Self, RenderError> {
        let bad = || matrix_invalid("bun_matrix_bad_marker");
        let number = |text: &str| text.parse::<u16>().map_err(|_| bad());
        let mut parsed = Vec::new();
        for entry in legs.split(',') {
            let fields: Vec<&str> = entry.split('/').collect();
            let [leg, project, shard, total] = fields.as_slice() else {
                return Err(bad());
            };
            parsed.push(BunLeg {
                leg: number(leg)?,
                project: (!project.is_empty()).then(|| (*project).to_owned()),
                shard: number(shard)?,
                total: number(total)?,
            });
        }
        let matrix = Self {
            legs: parsed,
            max_parallel: max_parallel.map(number).transpose()?,
        };
        matrix.validate()?;
        Ok(matrix)
    }
}

/// True when a job carries the Bun leg marker.
pub(crate) fn has_bun_matrix(job: &Job) -> bool {
    job.steps.iter().any(|step| {
        matches!(
            &step.kind,
            StepKind::Shell { env, .. }
                if env.contains_key(BUN_LEGS_ENV) || env.contains_key(BUN_MAX_PARALLEL_ENV)
        )
    })
}

/// Extract validated leg matrices from marked Bun suite jobs.
///
/// # Errors
///
/// A partial, malformed, non-canonical or repeated marker.
pub(crate) fn bun_matrices_of(
    jobs: &BTreeMap<String, Job>,
) -> Result<BTreeMap<String, BunLegMatrix>, RenderError> {
    let mut matrices = BTreeMap::new();
    for (id, job) in jobs {
        let mut found = None;
        for step in &job.steps {
            let StepKind::Shell { env, .. } = &step.kind else {
                continue;
            };
            let legs = env.get(BUN_LEGS_ENV);
            let cap = env.get(BUN_MAX_PARALLEL_ENV);
            match (legs, cap) {
                (None, None) => {}
                (None, Some(_)) => return Err(matrix_invalid("bun_matrix_marker_partial")),
                (Some(legs), cap) => {
                    if found.is_some() {
                        return Err(matrix_invalid("bun_matrix_marker_repeated"));
                    }
                    found = Some(BunLegMatrix::parse(legs, cap.map(String::as_str))?);
                }
            }
        }
        if let Some(matrix) = found {
            matrices.insert(id.clone(), matrix);
        }
    }
    Ok(matrices)
}

/// Attach `strategy:` for every sharded Bun suite job.
///
/// # Errors
///
/// A document without `jobs` or without a referenced job.
pub(crate) fn attach_bun_matrices(
    document: &mut Yaml,
    matrices: &BTreeMap<String, BunLegMatrix>,
) -> Result<(), RenderError> {
    if matrices.is_empty() {
        return Ok(());
    }
    let Yaml::Map(entries) = document else {
        return Err(matrix_invalid("matrix_without_document"));
    };
    let Some(Yaml::Map(jobs)) = entries
        .iter_mut()
        .find(|entry| entry.0 == "jobs")
        .map(|entry| &mut entry.1)
    else {
        return Err(matrix_invalid("matrix_without_jobs"));
    };
    for (id, matrix) in matrices {
        let include = matrix
            .legs
            .iter()
            .map(|leg| {
                let mut fields = vec![("leg".to_owned(), Yaml::Int(leg.leg.into()))];
                if let Some(project) = &leg.project {
                    fields.push(("project".to_owned(), Yaml::Str(project.clone())));
                }
                fields.push(("shard".to_owned(), Yaml::Int(leg.shard.into())));
                fields.push(("total".to_owned(), Yaml::Int(leg.total.into())));
                Yaml::Map(fields)
            })
            .collect();
        let mut strategy = vec![("fail-fast".to_owned(), Yaml::Bool(false))];
        if let Some(cap) = matrix.max_parallel {
            strategy.push(("max-parallel".to_owned(), Yaml::Int(cap.into())));
        }
        strategy.push((
            "matrix".to_owned(),
            Yaml::Map(vec![("include".to_owned(), Yaml::Seq(include))]),
        ));
        insert_job_key(jobs, id, "strategy", Yaml::Map(strategy))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "matrix_bun_tests.rs"]
mod tests;
