//! Handles "leaf types" - i.e. enums & scalars that don't have any nested fields.

use itertools::Itertools;

use super::{graph::GraphReader, inputs::InputObjects};
use crate::{
    Error,
    graph::{EnumDefinition, ScalarDefinition, TypeDefinition},
};

pub fn extract_leaf_types<'a>(
    graph: GraphReader<'a>,
    inputs: &InputObjects<'a>,
) -> Result<(Vec<EnumDefinition<'a>>, Vec<ScalarDefinition<'a>>), Error> {
    let mut leaf_types = graph
        .leaf_fields()
        .map(|field| field.field_definition().ty().name())
        .collect::<Vec<_>>();

    leaf_types.extend(
        graph
            .operations()
            .flat_map(|o| o.variable_definitions())
            .map(|variables| variables.ty().name()),
    );

    leaf_types.extend(
        graph
            .query_fragments()
            .flat_map(|fragment| fragment.leaf_fields())
            .map(|field| field.field_definition().ty().name()),
    );

    leaf_types.extend(inputs.required_input_types());

    let mut enums = Vec::new();
    let mut scalars = Vec::new();

    for name in leaf_types.into_iter().unique() {
        match graph.type_definition(name) {
            Some(TypeDefinition::Scalar(s)) => {
                if scalar_is_builtin(s) {
                    continue;
                }
                scalars.push(s);
            }
            Some(TypeDefinition::Enum(en)) => {
                enums.push(en);
            }
            _ => {}
        }
    }

    Ok((enums, scalars))
}

fn scalar_is_builtin(definition: ScalarDefinition<'_>) -> bool {
    matches!(
        definition.name(),
        "String" | "Int" | "Boolean" | "ID" | "Float"
    )
}
