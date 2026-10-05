use cynic_parser::{executable, type_system};

use super::{Argument, GraphReader, InputValueDefinition};

#[derive(Clone, Copy)]
pub struct Directive<'a> {
    pub(super) directive: executable::Directive<'a>,
    pub(super) definition: type_system::DirectiveDefinition<'a>,
    pub(super) reader: GraphReader<'a>,
}

impl<'a> Directive<'a> {
    pub fn name(&self) -> &'a str {
        self.directive.name()
    }

    pub fn arguments(&self) -> impl ExactSizeIterator<Item = Argument<'a>> {
        self.directive.arguments().map(move |argument| {
            let definition = self
                .definition
                .arguments()
                .find(|def| def.name() == argument.name())
                .unwrap_or_else(|| {
                    panic!("no argument named {} for @{}", argument.name(), self.name())
                });

            Argument {
                argument,
                definition: InputValueDefinition::directive_argument(definition, *self),
                reader: self.reader,
            }
        })
    }
}

impl PartialEq for Directive<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name() && self.arguments().eq(other.arguments())
    }
}

impl Eq for Directive<'_> {}
