use crate::{casings::CasingExt, graph::ScalarDefinition, processing::VariableStructs};

mod attr_output;
mod enums;
mod field;
mod indent;
mod inline_fragments;
mod input_object;
pub mod query_fragment;
mod variables_struct;

pub use {
    enums::Enum,
    indent::indented,
    inline_fragments::InlineFragments,
    input_object::{InputObject, InputObjectField},
    query_fragment::QueryFragment,
    variables_struct::VariablesStructForDisplay,
};

use field::Field;

pub struct Output<'a> {
    pub query_fragments: Vec<QueryFragment<'a>>,
    pub inline_fragments: Vec<InlineFragments>,
    pub input_objects: Vec<InputObject<'a>>,
    pub enums: Vec<Enum<'a>>,
    pub scalars: Vec<Scalar<'a>>,
    pub variable_structs: VariableStructs<'a>,
}

pub struct Scalar<'a> {
    pub definition: ScalarDefinition<'a>,
    pub schema_name: Option<String>,
}

impl std::fmt::Display for Scalar<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let graphql_name = self.definition.name();
        let rust_name = graphql_name.to_pascal_case();

        writeln!(f, "#[derive(cynic::Scalar, Debug, Clone)]")?;

        if graphql_name != rust_name {
            writeln!(f, "#[cynic(graphql_type = \"{}\")]", graphql_name)?;
        }

        writeln!(f, "pub struct {}(pub String);", rust_name)
    }
}
