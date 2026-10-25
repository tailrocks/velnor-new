use std::collections::BTreeMap;

use velnor_actions_contract::Job;

use super::{LaneShare, RenderContext, RenderError};

pub(crate) fn share_lanes(
    jobs: &BTreeMap<String, Job>,
    ctx: &RenderContext,
) -> Result<LaneShare, RenderError> {
    super::share_lanes_with_selection(jobs, ctx, None)
}
