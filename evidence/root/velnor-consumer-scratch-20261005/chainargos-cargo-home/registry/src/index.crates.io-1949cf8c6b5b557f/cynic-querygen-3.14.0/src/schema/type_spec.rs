use std::borrow::Cow;

use cynic_parser::{common::WrappingType, executable};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeSpec<'a> {
    pub(crate) name: Cow<'a, str>,
    pub(crate) contains_lifetime_a: bool,
}

impl<'a> TypeSpec<'a> {
    pub fn for_input_field(
        field: cynic_parser::type_system::InputValueDefinition<'a>,
        force_nullable: Option<bool>,
        needs_boxed: bool,
        is_subobject_with_lifetime: bool,
    ) -> TypeSpec<'static> {
        let wrappers = field.ty().wrappers().rev().collect::<Vec<_>>();
        input_type_spec_imp(
            field.ty().name(),
            wrappers,
            force_nullable.unwrap_or(true),
            needs_boxed,
            is_subobject_with_lifetime,
        )
    }

    pub fn for_executable_type(
        ty: executable::Type<'_>,
        is_subobject_with_lifetime: bool,
    ) -> TypeSpec<'static> {
        let wrappers = ty.wrappers().rev().collect::<Vec<_>>();
        input_type_spec_imp(ty.name(), wrappers, true, false, is_subobject_with_lifetime)
    }

    fn map(self, f: impl FnOnce(&str) -> String) -> TypeSpec<'static> {
        TypeSpec {
            name: Cow::Owned(f(&self.name)),
            contains_lifetime_a: self.contains_lifetime_a,
        }
    }
    pub(crate) fn lifetime<'b>(
        struct_type_specs: impl IntoIterator<Item = &'b Self>,
    ) -> &'static str
    where
        'a: 'b,
    {
        if struct_type_specs
            .into_iter()
            .any(|ts| ts.contains_lifetime_a)
        {
            "<'a>"
        } else {
            ""
        }
    }
}

fn input_type_spec_imp(
    name: &str,
    mut wrappers: Vec<WrappingType>,
    nullable: bool,
    needs_boxed: bool,
    is_subobject_with_lifetime: bool,
) -> TypeSpec<'static> {
    use crate::casings::CasingExt;

    if let Some(WrappingType::NonNull) = wrappers.last() {
        wrappers.pop();
        return input_type_spec_imp(
            name,
            wrappers,
            false,
            needs_boxed,
            is_subobject_with_lifetime,
        );
    }

    if nullable {
        return input_type_spec_imp(
            name,
            wrappers,
            false,
            needs_boxed,
            is_subobject_with_lifetime,
        )
        .map(|type_spec| format!("Option<{type_spec}>",));
    }

    match wrappers.pop() {
        Some(WrappingType::List) => {
            input_type_spec_imp(name, wrappers, true, false, is_subobject_with_lifetime)
                .map(|type_spec| format!("Vec<{type_spec}>",))
        }

        Some(WrappingType::NonNull) => panic!("NonNullType somehow got past an if let"),

        None => {
            let mut contains_lifetime_a = false;
            let mut name = match name {
                "Int" => Cow::Borrowed("i32"),
                "Float" => Cow::Borrowed("f64"),
                "Boolean" => Cow::Borrowed("bool"),
                "ID" => {
                    contains_lifetime_a = true;
                    Cow::Borrowed("&'a cynic::Id")
                }
                "String" => {
                    contains_lifetime_a = true;
                    Cow::Borrowed("&'a str")
                }
                _ => Cow::Owned({
                    let mut type_ = name.to_pascal_case();
                    if is_subobject_with_lifetime {
                        type_ += "<'a>";
                        contains_lifetime_a = true;
                    }
                    type_
                }),
            };

            if needs_boxed {
                name = Cow::Owned(format!("Box<{}>", name));
            }

            TypeSpec {
                name,
                contains_lifetime_a,
            }
        }
    }
}
