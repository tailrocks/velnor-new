mod argument;
mod definitions;
mod directive;
#[cfg(test)] // Dot is mostly for debugging, so lets not compile it outside of tests
mod dot;
mod fields;
mod fragments;
mod ingest;
mod operation;
mod selections;
mod transforms;
mod typed_value;
mod types;

use cynic_parser::{
    ExecutableDocument, TypeSystemDocument,
    executable::{
        self,
        ids::{FragmentDefinitionId, OperationDefinitionId},
    },
    type_system::{self, ids::DefinitionId},
};
use petgraph::{
    stable_graph::{EdgeIndex, NodeIndex, StableDiGraph},
    visit::{EdgeFiltered, EdgeRef as _},
};

#[expect(unused_imports)]
pub use self::{
    argument::Argument,
    definitions::{
        EnumDefinition, InputObjectDefinition, InputValueDefinition, InterfaceDefinition,
        ObjectDefinition, ScalarDefinition, TypeDefinition, UnionDefinition,
    },
    directive::Directive,
    fields::{Field, FieldTarget},
    fragments::{
        Fragment, FragmentId, InlineFragment, InlineFragmentId, QueryFragment, QueryFragmentId,
    },
    operation::Operation,
    selections::{Selection, SelectionTarget, Spread},
    typed_value::TypedValue,
    types::Type,
};

pub struct Graph(StableDiGraph<Node, Edge>);

impl Graph {
    pub fn new(
        executable: &ExecutableDocument,
        type_system: &TypeSystemDocument,
        typename_field: type_system::ids::FieldDefinitionId,
    ) -> Self {
        let mut builder = ingest::GraphBuilder::new(type_system, typename_field);

        ingest::ingest_executable_doc(&mut builder, executable);

        let mut this = Graph(builder.finish());

        transforms::transform_graph(executable, type_system, &mut this);

        this
    }

    pub fn reader<'a>(
        &'a self,
        executable: &'a ExecutableDocument,
        type_system: &'a TypeSystemDocument,
    ) -> GraphReader<'a> {
        GraphReader {
            graph: self,
            executable,
            type_system,
        }
    }

    fn query_nodes_topsorted(&self) -> Vec<NodeIndex> {
        let query_graph =
            EdgeFiltered::from_fn(&self.0, |edge| !matches!(edge.weight(), Edge::IsOfType));

        petgraph::algo::toposort(&query_graph, None).expect("query nodes shouldn't have cycles")
    }
}

#[derive(Clone, Copy)]
pub struct GraphReader<'a> {
    graph: &'a Graph,
    executable: &'a ExecutableDocument,
    type_system: &'a TypeSystemDocument,
}

impl<'a> GraphReader<'a> {
    pub fn type_definition(&self, name: &str) -> Option<TypeDefinition<'a>> {
        self.nodes()
            .filter_map(|node| {
                let Node::Type(_) = node.weight() else {
                    return None;
                };
                Some(TypeDefinition::new(node))
            })
            .find(|ty| ty.name() == name)
    }

    pub fn query_fragments(&self) -> impl Iterator<Item = QueryFragment<'a>> {
        self.graph
            .query_nodes_topsorted()
            .into_iter()
            .map(|index| NodeRef {
                reader: *self,
                index,
            })
            .filter(|node| matches!(node.weight(), Node::QueryFragment))
            .map(QueryFragment)
    }

    pub fn inline_fragments(&self) -> impl Iterator<Item = InlineFragment<'a>> {
        self.nodes()
            .filter(|node| matches!(node.weight(), Node::InlineFragment))
            .map(InlineFragment)
    }

    fn nodes(self) -> impl Iterator<Item = NodeRef<'a>> + 'a {
        self.graph.0.node_indices().map(move |index| NodeRef {
            reader: self,
            index,
        })
    }

    fn directive_definition(&self, name: &str) -> Option<type_system::DirectiveDefinition<'a>> {
        self.type_system.definitions().find_map(|def| match def {
            type_system::Definition::Directive(def) if def.name() == name => Some(def),
            _ => None,
        })
    }

    fn node_ref(&self, index: NodeIndex) -> NodeRef<'_> {
        NodeRef {
            reader: *self,
            index,
        }
    }

    #[expect(unused)]
    fn edge_ref(&self, index: EdgeIndex) -> EdgeRef<'_> {
        EdgeRef {
            reader: *self,
            index,
        }
    }
}

#[derive(Debug)]
enum Node {
    /// An Operation node.
    ///
    /// This appears in both the source and the input
    Operation(OperationDefinitionId),

    /// A QueryFragment in the output, generated from one or more selection sets in the input
    QueryFragment,
    /// An InlineFragment in the output, generated from one or more selection sets in the input
    InlineFragment,

    /// A type definition in the schema.  This contains a vector of all the definitions
    /// and extension for this type.
    Type(Vec<DefinitionId>),

    /// A named fragment from the source document.  This should not appear in output but may
    /// be used to generate names for fields in the output.
    NamedFragment(FragmentDefinitionId),

    LeafField,
}

#[derive(Clone, Copy, Debug)]
enum Edge {
    /// Edge that sits between operation/named fragment and their selection set
    HasSelectionSet,
    HasField {
        selection: executable::ids::FieldSelectionId,
        definition: type_system::ids::FieldDefinitionId,
        index: usize,
    },
    HasInlineSpread {
        fragment: executable::ids::InlineFragmentId,
        index: usize,
    },
    HasFragment {
        fragment: executable::ids::FragmentSpreadId,
        index: usize,
    },
    HasSyntheticSpread {
        index: usize,
    },

    IsOfType,
}

impl Edge {
    fn selection_index(&self) -> Option<usize> {
        match self {
            Edge::HasField { index, .. } => Some(*index),
            Edge::HasInlineSpread { index, .. } => Some(*index),
            Edge::HasFragment { index, .. } => Some(*index),
            Edge::HasSyntheticSpread { index } => Some(*index),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct NodeRef<'a> {
    reader: GraphReader<'a>,
    index: NodeIndex,
}

impl<'a> NodeRef<'a> {
    fn weight(&self) -> &'a Node {
        &self.reader.graph.0[self.index]
    }

    fn node_ref_for(&self, index: NodeIndex) -> NodeRef<'a> {
        NodeRef {
            reader: self.reader,
            index,
        }
    }

    fn incoming_edges(self) -> impl Iterator<Item = EdgeRef<'a>> + 'a {
        self.reader
            .graph
            .0
            .edges_directed(self.index, petgraph::Direction::Incoming)
            .map(move |edge| EdgeRef {
                reader: self.reader,
                index: edge.id(),
            })
    }

    fn outgoing_edges(self) -> impl Iterator<Item = EdgeRef<'a>> + 'a {
        self.reader
            .graph
            .0
            .edges_directed(self.index, petgraph::Direction::Outgoing)
            .map(move |edge| EdgeRef {
                reader: self.reader,
                index: edge.id(),
            })
    }
}

impl PartialEq for NodeRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.reader.graph, other.reader.graph) && self.index == other.index
    }
}

impl Eq for NodeRef<'_> {}

#[derive(Clone, Copy)]
struct EdgeRef<'a> {
    reader: GraphReader<'a>,
    index: EdgeIndex,
}

impl<'a> EdgeRef<'a> {
    fn weight(&self) -> &'a Edge {
        &self.reader.graph.0[self.index]
    }

    fn endpoints(&self) -> (NodeRef<'a>, NodeRef<'a>) {
        let (src, dest) = self
            .reader
            .graph
            .0
            .edge_endpoints(self.index)
            .expect("an edge ref has to have endpoints");

        (
            NodeRef {
                index: src,
                reader: self.reader,
            },
            NodeRef {
                index: dest,
                reader: self.reader,
            },
        )
    }

    fn source(&self) -> NodeRef<'a> {
        let (source, _) = self.endpoints();
        source
    }

    fn target(&self) -> NodeRef<'a> {
        let (_, dest) = self.endpoints();
        dest
    }
}

impl PartialEq for EdgeRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.reader.graph, other.reader.graph) && self.index == other.index
    }
}

impl Eq for EdgeRef<'_> {}
