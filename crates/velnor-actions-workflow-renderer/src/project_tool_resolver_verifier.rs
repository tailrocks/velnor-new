use velnor_actions_contract::workflow::RUST_VERIFIER_USES;

use crate::yaml::Yaml;

use super::scalar;

pub(super) fn verifier_action_ref(step: &Yaml) -> Option<&'static str> {
    let (Yaml::Map(fields)
    | Yaml::AnchoredMap {
        entries: fields, ..
    }) = step
    else {
        return None;
    };
    fields
        .iter()
        .find_map(|(key, value)| (key == "uses").then(|| scalar(value)))
        .flatten()
        .and_then(verifier_action_ref_name)
}

fn verifier_action_ref_name(uses: &str) -> Option<&'static str> {
    if RUST_VERIFIER_USES.contains(&uses) {
        Some("VELNOR_TOOL_RUST")
    } else if uses == crate::verifier_composite::VERIFY_JACTIONLINT_USES {
        Some("VELNOR_TOOL_JACTIONLINT")
    } else {
        None
    }
}
