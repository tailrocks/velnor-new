//! Renderer-owned workflow step output IDs.

use velnor_actions_contract::{Step, StepId};

use crate::yaml::Yaml;

/// Append the output ID authorized by a typed step identity.
pub(crate) fn push_step_id(entries: &mut Vec<(String, Yaml)>, step: &Step) {
    let Some(id) = step.id.map(StepId::as_str) else {
        return;
    };
    push_explicit_step_id(entries, id);
}

/// Append an output ID for a fixed generated action whose ID has no contract
/// enum variant yet.
pub(crate) fn push_explicit_step_id(entries: &mut Vec<(String, Yaml)>, id: &str) {
    entries.push(("id".to_owned(), Yaml::str(id.to_owned())));
}

#[cfg(test)]
mod tests {
    use super::push_step_id;
    use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

    use crate::yaml::Yaml;

    #[test]
    fn runtime_cache_identity_has_one_renderer_owned_output_id() {
        let mut entries = Vec::new();
        let step = Step {
            name: "presentation can vary".to_owned(),
            id: Some(StepId::PublishBaseline),
            role: Some(StepRole::BaselinePublisher),
            condition: None,
            kind: StepKind::Internal {
                operation: "publish-baseline-v1".to_owned(),
                env: std::collections::BTreeMap::new(),
            },
        };
        push_step_id(&mut entries, &step);
        assert_eq!(
            entries,
            vec![("id".to_owned(), Yaml::str("publish-baseline".to_owned()))]
        );
    }
}
