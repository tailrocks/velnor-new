//! Ordered common-run factoring for lane pairs with job-local MBX actions.

use velnor_actions_contract::{Job, Step, StepKind};

use crate::render::RenderContext;
use crate::tree::RenderedFile;
use crate::{RenderError, cache_steps};

type IndexedStep<'a> = (usize, &'a Step);

/// A local composite call replacing the indicated contiguous source run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedCall {
    pub before_step: usize,
    pub skip_steps: usize,
    pub uses: String,
}

/// Pair result: shared calls/files plus each lane's remaining job-local steps.
pub(crate) struct SharedPair {
    pub hosted_calls: Vec<SharedCall>,
    pub local_calls: Vec<SharedCall>,
    pub hosted_steps: Vec<Step>,
    pub local_steps: Vec<Step>,
    pub files: Vec<RenderedFile>,
}

/// Factor one validated lane pair, leaving MBX actions and elected saves local.
pub(crate) fn share_pair(
    logical: &str,
    hosted: &Job,
    local: &Job,
    ctx: &RenderContext,
) -> Result<SharedPair, RenderError> {
    if !same_job_policy(hosted, local) {
        return Err(lane_body_differs(logical));
    }
    if hosted.steps.iter().any(cache_steps::is_mbx_action)
        || local.steps.iter().any(cache_steps::is_mbx_action)
    {
        if !same_mbx_actions(hosted, local, &ctx.runs_on) {
            return Err(lane_body_differs(logical));
        }
        let matches = common_step_matches(hosted, local);
        let runs = share_mbx_runs(logical, hosted, ctx, matches)?;
        return Ok(SharedPair {
            hosted_calls: runs.hosted_calls,
            local_calls: runs.local_calls,
            hosted_steps: hosted.steps.clone(),
            local_steps: local.steps.clone(),
            files: runs.files,
        });
    }

    let (hosted_common, hosted_steps) = peel_saves(&hosted.steps);
    let (local_common, local_steps) = peel_saves(&local.steps);
    if hosted_common != local_common {
        return Err(lane_body_differs(logical));
    }
    let uses = format!("$/.github/actions/{logical}");
    let file = super::composite_file(logical, &hosted_common, ctx)?;
    let call = SharedCall {
        before_step: 0,
        skip_steps: 0,
        uses,
    };
    Ok(SharedPair {
        hosted_calls: vec![call.clone()],
        local_calls: vec![call],
        hosted_steps,
        local_steps,
        files: vec![file],
    })
}

fn lane_body_differs(logical: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("lane_body_differs:{logical}"))
}

fn common_step_matches(hosted: &Job, local: &Job) -> Vec<(usize, usize)> {
    let hosted_steps: Vec<IndexedStep<'_>> = hosted
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| !cache_steps::is_mbx_action(step) && !is_elected_save(step))
        .collect();
    let local_steps: Vec<IndexedStep<'_>> = local
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| !cache_steps::is_mbx_action(step) && !is_elected_save(step))
        .collect();
    ordered_matches(&hosted_steps, &local_steps)
}

fn ordered_matches(hosted: &[IndexedStep<'_>], local: &[IndexedStep<'_>]) -> Vec<(usize, usize)> {
    if hosted.len() == local.len()
        && hosted
            .iter()
            .zip(local)
            .all(|(hosted_step, local_step)| hosted_step.1 == local_step.1)
    {
        return hosted
            .iter()
            .zip(local)
            .map(|(hosted_step, local_step)| (hosted_step.0, local_step.0))
            .collect();
    }
    let mut matches = Vec::new();
    collect_matches(hosted, local, &mut matches);
    matches
}

fn collect_matches(
    hosted: &[IndexedStep<'_>],
    local: &[IndexedStep<'_>],
    matches: &mut Vec<(usize, usize)>,
) {
    if hosted.is_empty() || local.is_empty() {
        return;
    }
    let mut prefix_len = 0;
    while prefix_len < hosted.len()
        && prefix_len < local.len()
        && hosted[prefix_len].1 == local[prefix_len].1
    {
        matches.push((hosted[prefix_len].0, local[prefix_len].0));
        prefix_len += 1;
    }
    if prefix_len > 0 {
        collect_matches(&hosted[prefix_len..], &local[prefix_len..], matches);
        return;
    }
    if hosted.len() == 1 {
        if let Some(local_index) = local.iter().position(|(_, step)| *step == hosted[0].1) {
            matches.push((hosted[0].0, local[local_index].0));
        }
        return;
    }
    if local.len() == 1 {
        if let Some(hosted_index) = hosted.iter().position(|(_, step)| *step == local[0].1) {
            matches.push((hosted[hosted_index].0, local[0].0));
        }
        return;
    }
    let middle = hosted.len() / 2;
    let split = split_index(&hosted[..middle], &hosted[middle..], local);
    collect_matches(&hosted[..middle], &local[..split], matches);
    collect_matches(&hosted[middle..], &local[split..], matches);
}

fn split_index(
    hosted_left: &[IndexedStep<'_>],
    hosted_right: &[IndexedStep<'_>],
    local: &[IndexedStep<'_>],
) -> usize {
    let prefix_lengths = prefix_lengths(hosted_left, local);
    let suffix_lengths = suffix_lengths(hosted_right, local);
    let mut best_length = 0;
    let mut best_split = 0;
    for split in 0..=local.len() {
        let length = prefix_lengths[split] + suffix_lengths[split];
        if length > best_length {
            best_length = length;
            best_split = split;
        }
    }
    best_split
}

fn prefix_lengths(hosted: &[IndexedStep<'_>], local: &[IndexedStep<'_>]) -> Vec<usize> {
    let mut previous = vec![0; local.len() + 1];
    let mut current = vec![0; local.len() + 1];
    for (_, hosted_step) in hosted {
        current[0] = 0;
        for (index, (_, local_step)) in local.iter().enumerate() {
            current[index + 1] = if hosted_step == local_step {
                previous[index] + 1
            } else {
                previous[index + 1].max(current[index])
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous
}

fn suffix_lengths(hosted: &[IndexedStep<'_>], local: &[IndexedStep<'_>]) -> Vec<usize> {
    let mut previous = vec![0; local.len() + 1];
    let mut current = vec![0; local.len() + 1];
    for (_, hosted_step) in hosted.iter().rev() {
        current[local.len()] = 0;
        for index in (0..local.len()).rev() {
            current[index] = if hosted_step == &local[index].1 {
                previous[index + 1] + 1
            } else {
                previous[index].max(current[index + 1])
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous
}

struct SharedRuns {
    hosted_calls: Vec<SharedCall>,
    local_calls: Vec<SharedCall>,
    files: Vec<RenderedFile>,
}

fn share_mbx_runs(
    logical: &str,
    hosted: &Job,
    ctx: &RenderContext,
    matches: Vec<(usize, usize)>,
) -> Result<SharedRuns, RenderError> {
    let mut writer = SharedRunWriter::new(logical, ctx);
    let mut current_start = None;
    let mut previous = None;
    let mut common = Vec::new();
    for (hosted_index, local_index) in matches {
        if previous.is_some_and(|(prev_hosted, prev_local)| {
            hosted_index != prev_hosted + 1 || local_index != prev_local + 1
        }) {
            let start = current_start.take().ok_or_else(|| {
                RenderError::InvalidWorkflow(format!("lane_lcs_missing_run_start:{logical}"))
            })?;
            writer.append(start, &common)?;
            common.clear();
        }
        current_start.get_or_insert((hosted_index, local_index));
        common.push(hosted.steps[hosted_index].clone());
        previous = Some((hosted_index, local_index));
    }
    if let Some(start) = current_start {
        writer.append(start, &common)?;
    }
    Ok(writer.finish())
}

struct SharedRunWriter<'a> {
    logical: &'a str,
    ctx: &'a RenderContext,
    segment: usize,
    hosted_calls: Vec<SharedCall>,
    local_calls: Vec<SharedCall>,
    files: Vec<RenderedFile>,
}

impl<'a> SharedRunWriter<'a> {
    fn new(logical: &'a str, ctx: &'a RenderContext) -> Self {
        Self {
            logical,
            ctx,
            segment: 0,
            hosted_calls: Vec::new(),
            local_calls: Vec::new(),
            files: Vec::new(),
        }
    }

    fn append(
        &mut self,
        (hosted_start, local_start): (usize, usize),
        common: &[Step],
    ) -> Result<(), RenderError> {
        let file_name = format!("{}-shared-{}", self.logical, self.segment);
        let action_path = format!(
            ".github/actions/shared-lanes/{}/segment-{}",
            self.logical, self.segment
        );
        let uses = format!("$/{action_path}");
        let file = super::composite_file_at(&file_name, &action_path, common, self.ctx)?;
        let call = |before_step| SharedCall {
            before_step,
            skip_steps: common.len(),
            uses: uses.clone(),
        };
        self.hosted_calls.push(call(hosted_start));
        self.local_calls.push(call(local_start));
        self.files.push(file);
        self.segment += 1;
        Ok(())
    }

    fn finish(self) -> SharedRuns {
        SharedRuns {
            hosted_calls: self.hosted_calls,
            local_calls: self.local_calls,
            files: self.files,
        }
    }
}

fn peel_saves(steps: &[Step]) -> (Vec<Step>, Vec<Step>) {
    let mut common = Vec::new();
    let mut extra = Vec::new();
    for step in steps {
        if is_elected_save(step) {
            extra.push(step.clone());
        } else {
            common.push(step.clone());
        }
    }
    (common, extra)
}

fn is_elected_save(step: &Step) -> bool {
    step.name == crate::cache_steps::TOOLS_SAVE_NAME
        || step.name == crate::tofu_cache::TOFU_PROVIDERS_SAVE_NAME
}

fn same_job_policy(hosted: &Job, local: &Job) -> bool {
    hosted.timeout_minutes == local.timeout_minutes
        && hosted.condition == local.condition
        && hosted.permissions == local.permissions
        && hosted.environment == local.environment
}

fn same_mbx_actions(hosted: &Job, local: &Job, default_label: &str) -> bool {
    let hosted_actions: Vec<_> = hosted
        .steps
        .iter()
        .filter(|step| cache_steps::is_mbx_action(step))
        .collect();
    let local_actions: Vec<_> = local
        .steps
        .iter()
        .filter(|step| cache_steps::is_mbx_action(step))
        .collect();
    if hosted_actions.len() != local_actions.len() {
        return false;
    }
    let hosted_isolated = cache_steps::has_hosted_cache_target(hosted, default_label);
    hosted_actions
        .iter()
        .zip(local_actions)
        .all(|(hosted, local)| {
            let mut hosted = (*hosted).clone();
            let mut local = (*local).clone();
            let StepKind::Action {
                with: hosted_with, ..
            } = &mut hosted.kind
            else {
                return false;
            };
            let StepKind::Action {
                with: local_with, ..
            } = &mut local.kind
            else {
                return false;
            };
            if hosted_isolated {
                let generation = hosted_with.get("cache-generation");
                if !generation.is_some_and(|value| value.contains("-job-"))
                    || hosted_with.contains_key("backend")
                    || hosted_with.contains_key("cache-key")
                    || hosted_with.contains_key("restore-keys")
                    || hosted_with.contains_key("isolate-objects-cache")
                    || hosted_with.contains_key("cache-key-suffix")
                    || local_with.get("backend").map(String::as_str) != Some("local")
                    || local_with.get("github-cache-mode").map(String::as_str) != Some("objects")
                    || local_with.contains_key("cache-generation")
                    || local_with.contains_key("cache-key")
                    || local_with.contains_key("restore-keys")
                {
                    return false;
                }
                hosted_with.remove("cache-generation");
                hosted_with.remove("github-cache-mode");
                local_with.remove("backend");
                local_with.remove("github-cache-mode");
            }
            hosted == local
        })
}
