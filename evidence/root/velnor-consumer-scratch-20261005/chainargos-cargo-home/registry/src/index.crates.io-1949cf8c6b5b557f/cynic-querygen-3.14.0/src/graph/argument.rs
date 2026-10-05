use cynic_parser::executable;

use super::{GraphReader, InputValueDefinition, Type, TypedValue};

#[derive(Clone, Copy)]
pub struct Argument<'a> {
    pub(super) argument: executable::Argument<'a>,
    pub(super) definition: InputValueDefinition<'a>,
    pub(super) reader: GraphReader<'a>,
}

impl<'a> Argument<'a> {
    pub fn name(&self) -> &'a str {
        self.argument.name()
    }

    pub fn value(&self) -> TypedValue<'a> {
        let definition = self
            .reader
            .type_definition(self.definition.ty().name())
            .unwrap_or_else(|| panic!("unknown type: {}", self.definition.ty().name()));

        TypedValue::new(
            self.argument.value(),
            Type {
                definition,
                wrappers: self.definition.ty().wrappers().collect(),
            },
        )
    }
}

impl PartialEq for Argument<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.argument.name() == other.argument.name()
            && self.argument.value() == other.argument.value()
    }
}

impl Eq for Argument<'_> {}
