use cynic_parser::{ExecutableDocument, TypeSystemDocument};
use itertools::Itertools;
use petgraph::{
    Direction,
    stable_graph::NodeIndex,
    visit::{EdgeFiltered, EdgeRef},
};

use super::{Edge, Fragment, GraphReader, Node, NodeRef, Selection};

pub fn transform_graph(
    executable: &ExecutableDocument,
    type_system: &TypeSystemDocument,
    graph: &mut super::Graph,
) {
    deduplicate_fragments(executable, type_system, graph);
    lift_solo_spreads(executable, type_system, graph);

    prune_unused_leaves(graph);
}

fn deduplicate_fragments(
    executable: &ExecutableDocument,
    type_system: &TypeSystemDocument,
    graph: &mut super::Graph,
) {
    for root_index in topsorted_roots(graph) {
        let mut dfs = petgraph::visit::Dfs::new(&graph.0, root_index);

        loop {
            let query_graph =
                EdgeFiltered::from_fn(&graph.0, |edge| !matches!(edge.weight(), Edge::IsOfType));

            let Some(node) = dfs.next(&query_graph) else {
                break;
            };

            if !matches!(&graph.0[node], Node::QueryFragment | Node::InlineFragment) {
                continue;
            }

            // Get the type of this fragment
            let Some(ty_node) = graph
                .0
                .edges_directed(node, Direction::Outgoing)
                .find(|edge| matches!(edge.weight(), Edge::IsOfType))
                .map(|edge| edge.target())
            else {
                continue;
            };

            // Get other fragments of this type
            let fragments = graph
                .0
                .edges_directed(ty_node, Direction::Incoming)
                .filter(|edge| matches!(edge.weight(), Edge::IsOfType))
                .map(|edge| edge.source())
                .collect::<Vec<_>>();

            let mut merges = Merges::default();

            let reader = graph.reader(executable, type_system);

            for (left, right) in fragments.iter().tuple_combinations() {
                let left_fragment = create_fragment(reader, *left);
                let right_fragment = create_fragment(reader, *right);
                if left_fragment == right_fragment {
                    merges.add(*left, *right);
                }
            }

            if merges.0.is_empty() {
                continue;
            }

            for (target, sources) in merges.0 {
                for source in sources {
                    let new_edges = graph
                        .0
                        .edges_directed(source, Direction::Incoming)
                        .map(|edge| (edge.source(), *edge.weight()))
                        .collect::<Vec<_>>();

                    for (source, weight) in new_edges {
                        graph.0.add_edge(source, target, weight);
                    }

                    graph.0.remove_node(source);
                }
            }

            // We have potentially trashed the traversal so move back to the root and start again
            dfs.move_to(root_index);
        }
    }
}

fn topsorted_roots(graph: &mut super::Graph) -> Vec<NodeIndex> {
    graph
        .query_nodes_topsorted()
        .into_iter()
        .rev()
        .filter(|node| matches!(graph.0[*node], Node::Operation(_) | Node::NamedFragment(_)))
        .collect()
}

fn create_fragment(reader: GraphReader<'_>, index: NodeIndex) -> Fragment<'_> {
    Fragment::from_node(NodeRef { reader, index }).expect("could not create fragment from node")
}

#[derive(Default)]
struct Merges(Vec<(NodeIndex, Vec<NodeIndex>)>);

impl Merges {
    fn add(&mut self, left: NodeIndex, right: NodeIndex) {
        for (target, sources) in self.0.iter_mut() {
            if *target == left || sources.contains(&left) {
                sources.push(right);
                return;
            }
        }
        self.0.push((left, vec![right]));
    }
}

fn prune_unused_leaves(graph: &mut super::Graph) {
    let to_delete = graph
        .0
        .node_indices()
        .filter(|index| matches!(graph.0[*index], Node::LeafField))
        .filter(|index| {
            graph
                .0
                .edges_directed(*index, petgraph::Direction::Incoming)
                .next()
                .is_none()
        })
        .collect::<Vec<_>>();

    for index in to_delete {
        graph.0.remove_node(index);
    }
}

/// If a QueryFragment has a single spread in it we can just use the Fragment directly
fn lift_solo_spreads(
    executable: &ExecutableDocument,
    type_system: &TypeSystemDocument,
    graph: &mut super::Graph,
) {
    for root_index in topsorted_roots(graph) {
        let mut dfs = petgraph::visit::Dfs::new(&graph.0, root_index);

        loop {
            let query_graph =
                EdgeFiltered::from_fn(&graph.0, |edge| !matches!(edge.weight(), Edge::IsOfType));

            let Some(node) = dfs.next(&query_graph) else {
                break;
            };

            if !matches!(&graph.0[node], Node::QueryFragment) {
                continue;
            }
            let reader = graph.reader(executable, type_system);
            let fragment = super::QueryFragment(reader.node_ref(node));
            let selections = fragment.selections().collect::<Vec<_>>();

            let [Selection::Spread(spread)] = selections.as_slice() else {
                continue;
            };
            if fragment.type_definition().name() != spread.target().type_definition().name() {
                continue;
            }

            let target_index = spread.target().node_ref().index;

            let mut new_edges = vec![];
            let mut edges_to_delete = vec![];
            let mut can_delete_node = true;
            for edge in graph.0.edges_directed(node, petgraph::Direction::Incoming) {
                if !matches!(edge.weight(), Edge::HasField { .. }) {
                    can_delete_node = false;
                    continue;
                }
                edges_to_delete.push(edge.id());
                new_edges.push((edge.source(), *edge.weight()));
            }

            for (source, weight) in new_edges {
                graph.0.add_edge(source, target_index, weight);
            }

            if can_delete_node {
                graph.0.remove_node(node);
            } else {
                for index in edges_to_delete {
                    graph.0.remove_edge(index);
                }
            }
            dfs.move_to(root_index)
        }
    }
}
