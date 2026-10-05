use itertools::Itertools;
use petgraph::{stable_graph, visit::EdgeRef as _};

use crate::{casings::CasingExt, naming::Nameable};

use super::{Edge, Field, Node, NodeRef, Selection, TypeDefinition};

pub struct QueryFragment<'a>(pub(super) NodeRef<'a>);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct QueryFragmentId(stable_graph::NodeIndex);

impl<'a> QueryFragment<'a> {
    pub fn id(&self) -> QueryFragmentId {
        QueryFragmentId(self.0.index)
    }

    pub fn type_definition(&self) -> super::definitions::TypeDefinition<'a> {
        TypeDefinition::new(
            self.0
                .outgoing_edges()
                .find(|edge| matches!(edge.weight(), Edge::IsOfType))
                .expect("could not find IsOfType edge for QueryFragment")
                .target(),
        )
    }

    pub fn leaf_fields(self) -> impl Iterator<Item = Field<'a>> + 'a {
        self.0
            .outgoing_edges()
            .filter(move |edge| matches!(edge.target().weight(), Node::LeafField))
            .map(Field)
    }

    pub fn selections(&self) -> impl Iterator<Item = Selection<'a>> {
        self.0
            .outgoing_edges()
            .filter_map(move |edge| match edge.weight() {
                Edge::HasSelectionSet | Edge::IsOfType => None,
                Edge::HasField { index, .. } => Some((Selection::Field(Field(edge)), index)),
                Edge::HasInlineSpread { index, .. } | Edge::HasFragment { index, .. } => {
                    Some((Selection::Spread(super::Spread(edge)), index))
                }
                Edge::HasSyntheticSpread { index } => {
                    Some((Selection::Spread(super::Spread(edge)), index))
                }
            })
            .sorted_by_key(|(_, index)| **index)
            .map(|(selection, _)| selection)
    }
}

impl Nameable for QueryFragment<'_> {
    type Id = FragmentId;

    fn id(&self) -> Self::Id {
        QueryFragment::id(self).into()
    }

    fn requested_name(&self) -> String {
        requested_name(self.0, self.type_definition())
    }
}

impl PartialEq for QueryFragment<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
            || (self.type_definition().name() == other.type_definition().name()
                && self.selections().eq(other.selections()))
    }
}

impl Eq for QueryFragment<'_> {}

pub struct InlineFragment<'a>(pub(super) NodeRef<'a>);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct InlineFragmentId(stable_graph::NodeIndex);

impl<'a> InlineFragment<'a> {
    pub fn id(&self) -> QueryFragmentId {
        QueryFragmentId(self.0.index)
    }

    pub fn variants(&self) -> impl Iterator<Item = Fragment<'a>> {
        self.0
            .outgoing_edges()
            .filter_map(move |edge| Fragment::from_node(edge.target()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
    }

    pub fn type_definition(&self) -> super::definitions::TypeDefinition<'a> {
        TypeDefinition::new(
            self.0
                .outgoing_edges()
                .find(|edge| matches!(edge.weight(), Edge::IsOfType))
                .expect("could not find IsOfType edge for InlineFragment")
                .target(),
        )
    }
}

impl Nameable for InlineFragment<'_> {
    type Id = FragmentId;

    fn id(&self) -> Self::Id {
        InlineFragment::id(self).into()
    }

    fn requested_name(&self) -> String {
        requested_name(self.0, self.type_definition())
    }
}

impl PartialEq for InlineFragment<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
            || (self.type_definition().name() == other.type_definition().name()
                && self.variants().eq(other.variants()))
    }
}

impl Eq for InlineFragment<'_> {}

#[derive(PartialEq, Eq)]
pub enum Fragment<'a> {
    Query(QueryFragment<'a>),
    Inline(InlineFragment<'a>),
}

impl<'a> Fragment<'a> {
    pub fn id(&self) -> FragmentId {
        match self {
            Fragment::Query(inner) => inner.id().into(),
            Fragment::Inline(inner) => inner.id().into(),
        }
    }

    pub fn requested_name(&self) -> String {
        match self {
            Fragment::Query(query_fragment) => query_fragment.requested_name(),
            Fragment::Inline(inline_fragment) => inline_fragment.requested_name(),
        }
    }

    pub fn type_definition(&self) -> TypeDefinition<'a> {
        match self {
            Fragment::Query(inner) => inner.type_definition(),
            Fragment::Inline(inner) => inner.type_definition(),
        }
    }

    pub(super) fn from_node(node: NodeRef<'a>) -> Option<Self> {
        match node.weight() {
            Node::QueryFragment => Some(Fragment::Query(QueryFragment(node))),
            Node::InlineFragment => Some(Fragment::Inline(InlineFragment(node))),
            Node::NamedFragment(_) => {
                let selection_set_index = node
                    .reader
                    .graph
                    .0
                    .edges_directed(node.index, petgraph::Direction::Outgoing)
                    .find(|edge| matches!(edge.weight(), Edge::HasSelectionSet))
                    .expect("fragments should have a selection set")
                    .target();

                Self::from_node(node.node_ref_for(selection_set_index))
            }
            _ => None,
        }
    }

    pub(super) fn node_ref(&self) -> NodeRef {
        match self {
            Fragment::Query(inner) => inner.0,
            Fragment::Inline(inner) => inner.0,
        }
    }
}

impl<'a> From<QueryFragment<'a>> for Fragment<'a> {
    fn from(value: QueryFragment<'a>) -> Self {
        Fragment::Query(value)
    }
}

impl<'a> From<InlineFragment<'a>> for Fragment<'a> {
    fn from(value: InlineFragment<'a>) -> Self {
        Fragment::Inline(value)
    }
}

impl Nameable for Fragment<'_> {
    type Id = FragmentId;

    fn id(&self) -> Self::Id {
        self.id()
    }

    fn requested_name(&self) -> String {
        match self {
            Fragment::Query(inner) => inner.requested_name(),
            Fragment::Inline(inner) => inner.requested_name(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum FragmentId {
    Query(QueryFragmentId),
    Inline(InlineFragmentId),
}

impl From<QueryFragmentId> for FragmentId {
    fn from(value: QueryFragmentId) -> Self {
        FragmentId::Query(value)
    }
}

impl From<InlineFragmentId> for FragmentId {
    fn from(value: InlineFragmentId) -> Self {
        FragmentId::Inline(value)
    }
}

fn requested_name(fragment_node_ref: NodeRef<'_>, type_def: TypeDefinition<'_>) -> String {
    let executable = fragment_node_ref.reader.executable;
    fragment_node_ref
        .incoming_edges()
        .find_map(|edge| match edge.source().weight() {
            Node::Operation(op_id) => executable.read(*op_id).name(),
            Node::NamedFragment(fragment_id) => Some(executable.read(*fragment_id).name()),
            _ => None,
        })
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| type_def.name().to_string())
        .to_soft_pascal_case()
}
