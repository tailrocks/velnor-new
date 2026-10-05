use petgraph::{
    dot::Dot,
    stable_graph::NodeIndex,
    visit::{EdgeFiltered, EdgeRef as _, NodeFiltered},
};

use super::{Edge, EdgeRef, GraphReader, Node, NodeRef};

#[allow(unused)] // This mostly exists for debugging purposes, won't always be used
impl GraphReader<'_> {
    pub fn dbg_dot(&self) -> String {
        format!(
            "{:?}",
            Dot::with_attr_getters(
                &NodeFiltered::from_fn(
                    &EdgeFiltered::from_fn(&self.graph.0, |edge| !matches!(
                        edge.weight(),
                        Edge::IsOfType
                    )),
                    |index| { !matches!(self.graph.0[index], Node::Type(_)) }
                ),
                &[],
                &|_, edge| self.edge_attrs(edge),
                &|_, (index, weight)| self.node_attrs(index, weight),
            )
        )
    }

    fn edge_attrs(&self, edge: petgraph::stable_graph::EdgeReference<'_, Edge>) -> String {
        let edge_ref = EdgeRef {
            reader: *self,
            index: edge.id(),
        };
        match edge.weight() {
            Edge::HasSelectionSet => "".into(),
            Edge::HasField { .. } => {
                let field = super::Field(edge_ref).field_selection();
                format!(", name = {:?}, alias = {:?}", field.name(), field.alias())
            }
            Edge::HasInlineSpread { .. } => "".into(),
            Edge::HasFragment { .. } => "".into(),
            Edge::IsOfType => "".into(),
            Edge::HasSyntheticSpread { .. } => "".into(),
        }
    }

    fn node_attrs(&self, index: NodeIndex, weight: &Node) -> String {
        let node_ref = NodeRef {
            reader: *self,
            index,
        };
        match weight {
            Node::Operation(_) => format!(", name = {:?}", super::Operation(node_ref).name()),
            Node::QueryFragment => format!(
                ", type = \"{}\"",
                super::QueryFragment(node_ref).type_definition().name()
            ),
            Node::InlineFragment => format!(
                ", type = \"{}\"",
                super::QueryFragment(node_ref).type_definition().name()
            ),
            Node::Type(_) => {
                format!(
                    ", name = \"{}\"",
                    super::TypeDefinition::new(node_ref).name()
                )
            }
            Node::NamedFragment(_) => "".into(),
            Node::LeafField => "".into(),
        }
    }
}
