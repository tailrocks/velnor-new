use crate::lane_share::job_workflow_extract::{
    CALLER_INPUT_REQUIRED, CALLER_INPUT_TYPE, CALLER_INPUTS,
};

pub(super) fn assert_workflow_call_schema(workflow: &saphyr::Yaml<'_>) {
    let call = field(workflow, "on")
        .and_then(|trigger| field(trigger, "workflow_call"))
        .expect("the child declares a workflow_call trigger");
    assert!(field(call, "outputs").is_none());
    let inputs = field(call, "inputs").expect("the child declares workflow inputs");
    assert_eq!(
        inputs.as_mapping().map(saphyr::Mapping::len),
        Some(CALLER_INPUTS.len())
    );
    for input in CALLER_INPUTS {
        let definition = field(inputs, input.name).expect("caller input exists");
        assert_eq!(
            field(definition, "type").and_then(saphyr::Yaml::as_str),
            Some(CALLER_INPUT_TYPE)
        );
        assert_eq!(
            field(definition, "required").and_then(saphyr::Yaml::as_bool),
            Some(CALLER_INPUT_REQUIRED)
        );
    }
}

fn field<'node, 'source>(
    value: &'node saphyr::Yaml<'source>,
    name: &str,
) -> Option<&'node saphyr::Yaml<'source>> {
    value
        .as_mapping()?
        .iter()
        .find_map(|(key, value)| (key.as_str() == Some(name)).then_some(value))
}
