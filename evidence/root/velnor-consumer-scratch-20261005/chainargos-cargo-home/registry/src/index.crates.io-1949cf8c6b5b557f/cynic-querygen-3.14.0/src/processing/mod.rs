mod inputs;
mod leaf_types;
mod variables;

use crate::graph::{self, FragmentId, GraphReader, QueryFragment};
use inputs::InputObjects;

pub use variables::{VariableStruct, VariableStructField, VariableStructs};

use cynic_parser::{SchemaCoordinate, common::TypeWrappers};

use crate::{
    Error, OverrideMap,
    casings::CasingExt,
    graph::SelectionTarget,
    naming::{Nameable, Namer},
    output::{self, Output},
};

pub fn graph_to_output<'a>(
    graph: GraphReader<'a>,
    overrides: &OverrideMap,
) -> Result<Output<'a>, Error> {
    let input_objects = InputObjects::new(graph);

    let (mut enums, mut scalars) = leaf_types::extract_leaf_types(graph, &input_objects)?;

    enums.sort_by_key(|e| e.name());
    scalars.sort_by_key(|s| s.name());

    let mut namer = Namer::<FragmentId>::new();

    let variable_structs = variables::build_variable_structs(graph);

    let query_fragments = graph
        .query_fragments()
        .map(|fragment| make_query_fragment(fragment, &mut namer, &variable_structs, overrides))
        .collect::<Vec<_>>();

    let inline_fragments = graph
        .inline_fragments()
        .map(|fragment| make_inline_fragments(fragment, &mut namer, &variable_structs))
        .collect::<Vec<_>>();

    let input_objects = input_objects.processed_objects();

    let enums = enums
        .into_iter()
        .map(|definition| output::Enum {
            definition,
            schema_name: None,
        })
        .collect();

    let scalars = scalars
        .into_iter()
        .map(|definition| output::Scalar {
            definition,
            schema_name: None,
        })
        .collect();

    Ok(Output {
        query_fragments,
        inline_fragments,
        input_objects,
        enums,
        scalars,
        variable_structs,
    })
}

fn make_query_fragment<'a>(
    fragment: QueryFragment<'a>,
    namer: &mut Namer<FragmentId>,
    variable_struct_details: &VariableStructs<'a>,
    overrides: &OverrideMap,
) -> crate::output::QueryFragment<'a> {
    use self::graph::Selection;
    use crate::output::query_fragment::{OutputField, QueryFragment, RustOutputFieldType};

    let requested_fragment_name = fragment.requested_name();

    QueryFragment {
        fields: fragment
            .selections()
            .map(|selection| {
                let type_name_override = match (selection.target(), &selection) {
                    (SelectionTarget::Fragment(fragment), _) => Some(namer.name_subject(&fragment)),
                    (SelectionTarget::Scalar(_), Selection::Field(field)) => overrides
                        // Check for field-level type overrides using the requested fragment name (before incrementing suffix),
                        // and un-aliased field name in the schema.
                        .get(&SchemaCoordinate::member(
                            requested_fragment_name.clone(),
                            field.field_selection().name(),
                        ))
                        .map(|o| o.to_string()),
                    (SelectionTarget::Scalar(_), _) => {
                        unreachable!("scalars can only appear on field selections")
                    }
                };

                match selection {
                    Selection::Field(field) => {
                        let field_selection = field.field_selection();
                        let name = field_selection.name();
                        let alias = field_selection.alias();
                        let field_ty = field.field_definition().ty();
                        OutputField {
                            selection,
                            name: alias.unwrap_or(name).into(),
                            rename: alias
                                .map(|_| {
                                    // If we have an alias then we need a rename attr
                                    name
                                })
                                // Otherwise we only need one if a camelcase roundtrip would be lossy
                                .or_else(|| {
                                    (name.to_snake_case().to_camel_case() != name).then_some(name)
                                }),
                            field_type: RustOutputFieldType {
                                name: type_name_override
                                    .unwrap_or_else(|| field_ty.name().to_pascal_case()),
                                wrappers: field_ty.wrappers().collect(),
                            },
                        }
                    }
                    Selection::Spread(spread) => {
                        let target_name = namer.name_subject(&spread.target());

                        OutputField {
                            selection,
                            name: target_name.to_snake_case().into(),
                            rename: None,
                            field_type: RustOutputFieldType {
                                name: target_name,
                                // Spreads can't have an Option wrapper so make them non-null
                                wrappers: TypeWrappers::none().wrap_non_null(),
                            },
                        }
                    }
                }
            })
            .collect(),
        variable_struct_name: variable_struct_details
            .variables_name_for_fragment(fragment.id())
            .map(ToOwned::to_owned),

        name: namer.name_subject(&fragment),
        target_type: fragment.type_definition().name().to_string(),
        schema_name: None,
    }
}

fn make_inline_fragments<'a>(
    inline_fragment: graph::InlineFragment<'a>,
    namer: &mut Namer<FragmentId>,
    variable_structs: &VariableStructs<'a>,
) -> crate::output::InlineFragments {
    crate::output::InlineFragments {
        inner_type_names: inline_fragment
            .variants()
            .map(|fragment| namer.name_subject(&fragment))
            .collect(),
        target_type: inline_fragment.type_definition().name().into(),
        variable_struct_name: variable_structs
            .variables_name_for_fragment(inline_fragment.id())
            .map(ToOwned::to_owned),
        name: namer.name_subject(&inline_fragment),
        schema_name: None,
    }
}
