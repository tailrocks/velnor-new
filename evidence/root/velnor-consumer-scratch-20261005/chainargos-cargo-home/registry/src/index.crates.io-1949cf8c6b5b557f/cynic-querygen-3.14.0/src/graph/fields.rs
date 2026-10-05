use cynic_parser::executable::FieldSelection;

use super::{
    Argument, Directive, Edge, EdgeRef, Fragment, InlineFragment, Node, Type, TypeDefinition,
    definitions::FieldContainer, definitions::FieldDefinition, fragments::QueryFragment,
};

impl<'a> super::GraphReader<'a> {
    pub fn leaf_fields(&self) -> impl Iterator<Item = Field<'a>> {
        self.nodes()
            .filter(|node| matches!(node.weight(), Node::LeafField))
            .map(|node| {
                let edge = node
                    .incoming_edges()
                    .next()
                    .expect("LeafField node must have at least one incoming edge");

                Field(edge)
            })
    }
}

#[derive(Clone, Copy)]
pub struct Field<'a>(pub(super) EdgeRef<'a>);

impl<'a> Field<'a> {
    pub fn field_selection(&self) -> FieldSelection<'a> {
        let Edge::HasField { selection, .. } = &self.0.weight() else {
            panic!("invalid EdgeRef for Field");
        };

        self.0.reader.executable.read(*selection)
    }

    pub fn container(&self) -> QueryFragment<'a> {
        QueryFragment(self.0.source())
    }

    pub fn field_definition(&self) -> FieldDefinition<'a> {
        let Edge::HasField { definition, .. } = &self.0.weight() else {
            panic!("invalid EdgeRef for Field");
        };

        let field = self.0.reader.type_system.read(*definition);
        FieldDefinition {
            field,
            container: match self.container().type_definition() {
                TypeDefinition::Object(inner) => FieldContainer::Object(inner),
                TypeDefinition::Interface(inner) => FieldContainer::Interface(inner),
                TypeDefinition::Union(inner) => {
                    // Unions mostly don't have fields, but they do still have __typename
                    FieldContainer::Union(inner)
                }
                _ => {
                    panic!("invalid container type for field")
                }
            },
        }
    }

    pub fn arguments(&self) -> impl ExactSizeIterator<Item = Argument<'a>> {
        let field_def = self.field_definition();
        self.field_selection().arguments().map(move |argument| {
            let definition = field_def
                .argument_definitions()
                .find(|def| def.name() == argument.name())
                .unwrap_or_else(|| {
                    panic!(
                        "no argument named {} for {}.{}",
                        argument.name(),
                        self.container().type_definition().name(),
                        self.field_selection().name()
                    )
                });

            Argument {
                argument,
                definition,
                reader: self.0.reader,
            }
        })
    }

    pub fn directives(&self) -> impl ExactSizeIterator<Item = Directive<'a>> {
        self.field_selection().directives().map(move |directive| {
            let definition = self
                .0
                .reader
                .directive_definition(directive.name())
                .unwrap_or_else(|| panic!("could not directive @{}", directive.name()));

            Directive {
                directive,
                definition,
                reader: self.0.reader,
            }
        })
    }

    pub fn target_fragment(&self) -> Option<Fragment<'a>> {
        match self.0.target().weight() {
            Node::QueryFragment => Some(Fragment::Query(QueryFragment(self.0.target()))),
            Node::InlineFragment => Some(Fragment::Inline(InlineFragment(self.0.target()))),
            _ => None,
        }
    }

    pub fn target(&self) -> FieldTarget<'a> {
        self.target_fragment()
            .map(FieldTarget::Fragment)
            .unwrap_or_else(|| FieldTarget::Scalar(self.field_definition().ty()))
    }
}

impl PartialEq for Field<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
            || (self.field_selection().name() == other.field_selection().name()
                && self.field_selection().alias() == other.field_selection().alias()
                && self.arguments().eq(other.arguments())
                && self.directives().eq(other.directives()))
                && self.target() == other.target()
    }
}

#[derive(PartialEq, Eq)]
pub enum FieldTarget<'a> {
    Scalar(Type<'a>),
    Fragment(Fragment<'a>),
}
