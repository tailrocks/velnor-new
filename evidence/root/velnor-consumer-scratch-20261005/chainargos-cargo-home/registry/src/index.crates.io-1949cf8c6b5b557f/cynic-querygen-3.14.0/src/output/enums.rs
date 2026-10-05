use crate::{casings::CasingExt, graph::EnumDefinition, output::attr_output::Attributes};
use std::fmt::Write;

use super::indented;

pub struct Enum<'a> {
    pub definition: EnumDefinition<'a>,

    pub schema_name: Option<String>,
}

impl std::fmt::Display for Enum<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let type_name = self.definition.name();

        writeln!(f, "#[derive(cynic::Enum, Clone, Copy, Debug)]")?;

        let mut attributes = Attributes::new("cynic");
        if type_name != type_name.to_pascal_case() {
            attributes.push(format!("graphql_type = \"{type_name}\""));
        }
        if let Some(schema_name) = &self.schema_name {
            attributes.push(format!("schema = \"{schema_name}\""));
        }

        write!(f, "{attributes}")?;
        writeln!(f, "pub enum {} {{", type_name.to_pascal_case())?;

        for variant in self.definition.values() {
            let mut f = indented(f, 4);

            let value = variant.value();
            if value.to_pascal_case().to_screaming_snake_case() != value {
                // If a pascal -> screaming snake casing roundtrip is not lossless
                // we need to explicitly rename this variant
                writeln!(f, "#[cynic(rename = \"{value}\")]")?;
            }

            writeln!(f, "{},", value.to_pascal_case())?;
        }
        writeln!(f, "}}")
    }
}
