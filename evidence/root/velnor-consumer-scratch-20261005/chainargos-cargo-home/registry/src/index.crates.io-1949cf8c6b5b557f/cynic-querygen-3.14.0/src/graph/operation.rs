use cynic_parser::executable::{self, OperationDefinition};

use super::{Edge, Node, NodeRef, fragments::QueryFragment};

impl<'a> super::GraphReader<'a> {
    pub fn operations(&self) -> impl Iterator<Item = Operation<'a>> {
        self.nodes()
            .filter(|node| matches!(node.weight(), Node::Operation(_)))
            .map(Operation)
    }
}

pub struct Operation<'a>(pub(super) NodeRef<'a>);

impl<'a> Operation<'a> {
    pub fn name(&self) -> Option<&'a str> {
        self.definition().name()
    }

    pub fn variable_definitions(&self) -> executable::Iter<'a, executable::VariableDefinition<'a>> {
        self.definition().variable_definitions()
    }

    pub fn root(&self) -> QueryFragment<'a> {
        self.0
            .outgoing_edges()
            .find(|edge| matches!(edge.weight(), Edge::HasSelectionSet))
            .map(|edge| QueryFragment(edge.target()))
            .unwrap()
    }

    fn definition(&self) -> OperationDefinition<'a> {
        let Node::Operation(id) = self.0.weight() else {
            panic!("malformed Operation")
        };
        self.0.reader.executable.read(*id)
    }
}
