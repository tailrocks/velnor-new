use std::iter;

use super::{Directive, Edge, EdgeRef, Field, Type, fields::FieldTarget, fragments::Fragment};

#[derive(Clone, Copy, PartialEq)]
pub enum Selection<'a> {
    Field(Field<'a>),
    Spread(Spread<'a>),
}

impl<'a> Selection<'a> {
    pub fn target(&self) -> SelectionTarget<'a> {
        match self {
            Selection::Field(field) => match field.target() {
                FieldTarget::Scalar(ty) => SelectionTarget::Scalar(ty),
                FieldTarget::Fragment(fragment) => SelectionTarget::Fragment(fragment),
            },
            Selection::Spread(spread) => SelectionTarget::Fragment(spread.target()),
        }
    }
}

impl std::fmt::Debug for Selection<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Field(_) => f.debug_tuple("Field").finish_non_exhaustive(),
            Self::Spread(_) => f.debug_tuple("Spread").finish_non_exhaustive(),
        }
    }
}

pub enum SelectionTarget<'a> {
    Fragment(Fragment<'a>),
    #[expect(dead_code)]
    Scalar(Type<'a>),
}

#[derive(Clone, Copy)]
pub struct Spread<'a>(pub(super) EdgeRef<'a>);

impl<'a> Spread<'a> {
    pub fn directives(self) -> Box<dyn Iterator<Item = Directive<'a>> + 'a> {
        Box::new(
            match self.0.weight() {
                Edge::HasInlineSpread { fragment, .. } => {
                    self.0.reader.executable.read(*fragment).directives()
                }
                Edge::HasFragment { fragment, .. } => {
                    self.0.reader.executable.read(*fragment).directives()
                }
                Edge::HasSyntheticSpread { .. } => return Box::new(iter::empty()),
                _ => unreachable!("Spread should point at an inline or fragment"),
            }
            .map(move |directive| {
                let definition = self
                    .0
                    .reader
                    .directive_definition(directive.name())
                    .unwrap_or_else(|| panic!("could not find directive @{}", directive.name()));

                Directive {
                    directive,
                    definition,
                    reader: self.0.reader,
                }
            }),
        )
    }

    pub fn target(&self) -> Fragment<'a> {
        Fragment::from_node(self.0.target()).expect("could not construct Fragment from node")
    }
}

impl PartialEq for Spread<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
            || (self.target() == other.target() && self.directives().eq(other.directives()))
    }
}

impl Eq for Spread<'_> {}
