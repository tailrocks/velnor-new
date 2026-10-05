use std::{
    borrow::Cow,
    fmt::{self, Write},
};

use cynic_parser::common::{TypeWrappers, WrappingType};

use crate::{
    casings::CasingExt,
    graph::{self, TypedValue},
    output::{attr_output::Attributes, field::rust_field_name},
    schema::TypeSpec,
};

use super::indented;

#[derive(Debug)]
pub struct QueryFragment<'a> {
    pub fields: Vec<OutputField<'a>>,
    pub target_type: String,
    pub variable_struct_name: Option<String>,
    pub schema_name: Option<String>,

    pub name: String,
}

impl std::fmt::Display for QueryFragment<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "#[derive(cynic::QueryFragment, Debug)]")?;

        let mut attributes = Attributes::new("cynic");
        if self.target_type != self.name {
            attributes.push(format!("graphql_type = \"{}\"", self.target_type));
        }

        if let Some(name) = &self.variable_struct_name {
            attributes.push(format!("variables = \"{name}\""));
        }

        if let Some(schema_name) = &self.schema_name {
            attributes.push(format!("schema = \"{schema_name}\""))
        }

        write!(f, "{attributes}")?;
        writeln!(f, "pub struct {} {{", self.name)?;
        for field in &self.fields {
            write!(indented(f, 4), "{}", field)?;
        }

        writeln!(f, "}}")
    }
}

#[derive(Debug)]
pub struct OutputField<'a> {
    pub selection: crate::graph::Selection<'a>,
    pub name: Cow<'a, str>,
    pub rename: Option<&'a str>,
    pub field_type: RustOutputFieldType,
}

impl std::fmt::Display for OutputField<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self.name.to_snake_case();
        let type_spec = TypeSpec {
            name: self.field_type.to_string().into(),
            contains_lifetime_a: false,
        };
        let mut output = super::Field::new(&name, &type_spec);

        if let Some(rename) = self.rename {
            output.add_rename(rename);
        }

        match &self.selection {
            graph::Selection::Field(field) => {
                if field.arguments().len() != 0 {
                    let arguments_string = field
                        .arguments()
                        .map(|arg| format!("{}: {}", arg.name(), arg.value().into_literal()))
                        .collect::<Vec<_>>()
                        .join(", ");

                    writeln!(f, "#[arguments({})]", arguments_string)?;
                }
                if field.directives().len() != 0 {
                    let directive_string = field
                        .directives()
                        .map(|directive| {
                            let name = directive.name();
                            if directive.arguments().len() == 0 {
                                return name.to_string();
                            }
                            let argument_strings = directive
                                .arguments()
                                .map(|argument| {
                                    format!(
                                        "{}: {}",
                                        argument.name(),
                                        argument.value().into_literal()
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(", ");

                            format!("{name}({argument_strings})")
                        })
                        .collect::<Vec<_>>()
                        .join(", ");

                    writeln!(f, "#[directives({directive_string})]")?;
                }
            }
            graph::Selection::Spread(_) => {
                output.add_spread();
            }
        }

        write!(f, "{}", output)
    }
}

/// An OutputFieldType that has been given a rust-land name.  Allows for
/// the fact that there may be several rust structs that refer to the same
/// schema type.
#[derive(Debug)]
pub struct RustOutputFieldType {
    pub name: String,
    pub wrappers: TypeWrappers,
}

impl fmt::Display for RustOutputFieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut nullable = true;
        let mut n_brackets = 0;
        for wrapper in self.wrappers.iter() {
            match wrapper {
                WrappingType::NonNull => {
                    nullable = false;
                }
                WrappingType::List => {
                    if nullable {
                        write!(f, "Option<")?;
                        n_brackets += 1;
                    }
                    nullable = true;
                    n_brackets += 1;
                    write!(f, "Vec<")?;
                }
            }
        }
        if nullable {
            write!(f, "Option<")?;
            n_brackets += 1;
        }
        match self.name.as_ref() {
            "Int" => write!(f, "i32")?,
            "Float" => write!(f, "f64")?,
            "Boolean" => write!(f, "bool")?,
            // The actual GraphQL type here is Id but we've already pascal cased it here...
            "Id" => write!(f, "cynic::Id")?,
            name => write!(f, "{name}")?,
        }

        for _ in 0..n_brackets {
            write!(f, ">")?;
        }

        Ok(())
    }
}

impl TypedValue<'_> {
    fn into_literal(self) -> String {
        match self {
            TypedValue::Variable(variable) => {
                let name = variable.name().to_snake_case();
                let name = rust_field_name(&name);
                format!("${name}")
            }
            TypedValue::Int(inner) => inner.value.to_string(),
            TypedValue::Float(inner) => inner.value.to_string(),
            TypedValue::String(inner) => {
                let s = inner.value.as_str();
                if string_needs_raw_literal(s) {
                    format!("r#\"{s}\"#")
                } else {
                    format!("\"{s}\"")
                }
            }
            TypedValue::Boolean(inner) => inner.value.to_string(),
            TypedValue::Null(_) => "null".into(),
            TypedValue::Enum(inner) => {
                format!("\"{}\"", inner.value.as_str())
            }
            TypedValue::List(list) => {
                let inner = list
                    .items()
                    .map(|v| v.into_literal())
                    .collect::<Vec<_>>()
                    .join(", ");

                format!("[{inner}]")
            }
            TypedValue::Object(obj) => {
                let fields = obj
                    .fields()
                    .map(|field| format!("{}: {}", field.name(), field.value().into_literal()))
                    .collect::<Vec<_>>();

                let fields = fields.join(", ");

                format!("{{ {fields} }}")
            }
        }
    }
}

fn string_needs_raw_literal(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_control() || c == '"')
}

#[cfg(test)]
mod tests {
    use cynic_parser::common::TypeWrappers;

    use crate::output::query_fragment::RustOutputFieldType;

    #[test]
    fn rust_output_field_type_display() {
        assert_eq!(
            RustOutputFieldType {
                name: "Id".into(),
                wrappers: TypeWrappers::default().wrap_non_null()
            }
            .to_string(),
            "cynic::Id"
        );

        assert_eq!(
            RustOutputFieldType {
                name: "Foo".into(),
                wrappers: TypeWrappers::default()
            }
            .to_string(),
            "Option<Foo>"
        );

        assert_eq!(
            RustOutputFieldType {
                name: "Foo".into(),
                wrappers: TypeWrappers::default().wrap_list()
            }
            .to_string(),
            "Option<Vec<Option<Foo>>>"
        );

        assert_eq!(
            RustOutputFieldType {
                name: "Foo".into(),
                wrappers: TypeWrappers::default().wrap_non_null().wrap_list()
            }
            .to_string(),
            "Option<Vec<Foo>>"
        );

        assert_eq!(
            RustOutputFieldType {
                name: "Foo".into(),
                wrappers: TypeWrappers::default()
                    .wrap_non_null()
                    .wrap_list()
                    .wrap_non_null()
            }
            .to_string(),
            "Vec<Foo>"
        );
    }
}
