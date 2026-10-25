use velnor_actions_contract::Job;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CallerInput {
    pub(crate) name: &'static str,
    pub(crate) expression: &'static str,
}

pub(crate) const CALLER_INPUT_TYPE: &str = "string";
pub(crate) const CALLER_INPUT_REQUIRED: bool = true;

pub(crate) const CALLER_INPUTS: [CallerInput; 5] = [
    CallerInput {
        name: "plan_result",
        expression: "${{ needs.plan.result }}",
    },
    CallerInput {
        name: "qualification_campaign",
        expression: "${{ needs.plan.outputs.qualification_campaign }}",
    },
    CallerInput {
        name: "run_key_attempt",
        expression: "${{ needs.plan.outputs.plan_attempt || github.run_attempt }}",
    },
    CallerInput {
        name: "sources_key",
        expression: "${{ needs.plan.outputs.sources_key }}",
    },
    CallerInput {
        name: "sources_restore_keys",
        expression: "${{ needs.plan.outputs.sources_restore_keys }}",
    },
];

/// Bind only source-cache outputs consumed by this typed source job.
pub(crate) fn source_bound_caller_inputs(source: &Job) -> [CallerInput; 5] {
    let usage = crate::cache_plan_outputs::source_cache_usage(source);
    let mut inputs = CALLER_INPUTS;
    for input in &mut inputs {
        match input.name {
            "sources_key" if !usage.sources_key => input.expression = "",
            "sources_restore_keys" if !usage.sources_restore_keys => input.expression = "",
            _ => {}
        }
    }
    inputs
}
