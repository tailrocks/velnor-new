//! Native manual dispatch serialization.
use crate::yaml::Yaml;
use velnor_actions_contract::workflow::ir::{DispatchInput, DispatchInputType};

/// Render one typed dispatch input, preserving native Boolean defaults.
pub(super) fn dispatch_input_to_yaml(input: &DispatchInput) -> Yaml {
    let mut fields = vec![
        (
            "type".to_owned(),
            Yaml::str(input.input_type.as_str().to_owned()),
        ),
        ("required".to_owned(), Yaml::Bool(input.required)),
    ];
    if let Some(default) = &input.default {
        let value = match input.input_type {
            DispatchInputType::String | DispatchInputType::Choice => Yaml::str(default.clone()),
            DispatchInputType::Boolean => Yaml::Bool(default == "true"),
        };
        fields.push(("default".to_owned(), value));
    }
    if let Some(description) = &input.description {
        fields.push(("description".to_owned(), Yaml::str(description.clone())));
    }
    if !input.options.is_empty() {
        fields.push((
            "options".to_owned(),
            Yaml::Seq(input.options.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    Yaml::Map(fields)
}

#[cfg(test)]
mod tests {
    use super::{DispatchInput, DispatchInputType, dispatch_input_to_yaml};
    use crate::yaml::render_yaml;

    #[test]
    fn native_dispatch_yaml_preserves_empty_string_choice_and_false() {
        let mut input = DispatchInput {
            name: "version".to_owned(),
            input_type: DispatchInputType::String,
            required: false,
            description: None,
            options: Vec::new(),
            default: Some(String::new()),
        };
        assert!(render_yaml(&dispatch_input_to_yaml(&input)).contains("default: \"\""));
        input.input_type = DispatchInputType::Boolean;
        input.default = Some("false".to_owned());
        assert!(render_yaml(&dispatch_input_to_yaml(&input)).contains("default: false"));
        input.input_type = DispatchInputType::Choice;
        input.default = Some("false".to_owned());
        input.options = vec!["false".to_owned(), "stable".to_owned()];
        input.description = Some("Choose channel".to_owned());
        let text = render_yaml(&dispatch_input_to_yaml(&input));
        assert!(text.contains("type: choice"));
        assert!(text.contains("default: \"false\""));
        assert!(text.contains("description: Choose channel"));
        assert!(text.contains("options:\n  - \"false\"\n  - stable"));
    }
}
