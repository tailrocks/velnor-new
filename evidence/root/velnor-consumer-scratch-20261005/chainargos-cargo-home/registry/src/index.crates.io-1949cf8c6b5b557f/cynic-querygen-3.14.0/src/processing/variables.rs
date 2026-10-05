//! Generation of variable structs

use std::collections::{HashMap, HashSet};

use cynic_parser::executable::VariableDefinition;
use petgraph::{
    Direction,
    graph::NodeIndex,
    visit::{DfsPostOrder, EdgeRef, IntoNodeReferences, NodeRef},
};

use super::graph::{self, Fragment, FragmentId, GraphReader};
use crate::{casings::CasingExt, graph::TypedValue, naming::Nameable};

#[derive(Default)]
pub struct VariableStructs<'a> {
    pub structs: Vec<VariableStruct<'a>>,
    mapping_sorted: Vec<(FragmentId, usize)>,
}

impl VariableStructs<'_> {
    pub fn variables_name_for_fragment(&self, id: impl Into<FragmentId>) -> Option<&str> {
        let mapping_index = self
            .mapping_sorted
            .binary_search_by_key(&id.into(), |item| item.0)
            .ok()?;

        Some(&self.structs[self.mapping_sorted[mapping_index].1].name)
    }
}

pub struct VariableStruct<'a> {
    pub name: String,
    pub fields: Vec<VariableStructField<'a>>,
}

pub enum VariableStructField<'a> {
    Variable(VariableDefinition<'a>),
    NestedStruct { name: String },
}

impl VariableStructField<'_> {
    pub fn name(&self) -> &str {
        match self {
            VariableStructField::Variable(typed_variable_value) => typed_variable_value.name(),
            VariableStructField::NestedStruct { name } => name,
        }
    }
}

pub fn build_variable_structs(graph: GraphReader<'_>) -> VariableStructs<'_> {
    let mut variable_graph = VariableGraph::new();

    for operation in graph.operations() {
        let variables = operation
            .variable_definitions()
            .map(|def| (def.name(), def))
            .collect::<HashMap<_, _>>();

        let operation_index = variable_graph.add_node(VariableGraphNode::Operation);

        add_fragment_to_variable_graph(
            &mut variable_graph,
            &variables,
            format!(
                "{}Variables",
                operation.name().unwrap_or("Query").to_soft_pascal_case()
            ),
            operation_index,
            operation.root().into(),
        );
    }

    remove_unused_structs(&mut variable_graph);
    simplify_variable_graph(&mut variable_graph);

    // eprintln!("{:?}", petgraph::dot::Dot::new(&variable_graph));

    let mut mapping = vec![];
    let mut structs = vec![];

    for node in variable_graph.node_references() {
        let VariableGraphNode::VariableStruct { name } = node.weight() else {
            continue;
        };

        // We need to deduplicate variables
        let mut seen_variables = HashSet::new();

        structs.push(VariableStruct {
            name: name.clone(),
            fields: vec![],
        });

        let variable_struct_index = structs.len() - 1;
        let variable_struct = structs.last_mut().unwrap();

        for edge in variable_graph.edges_directed(node.id(), Direction::Outgoing) {
            match &variable_graph[edge.target()] {
                VariableGraphNode::Variable(variable) if seen_variables.insert(variable.name()) => {
                    variable_struct
                        .fields
                        .push(VariableStructField::Variable(*variable))
                }
                VariableGraphNode::VariableStruct { name: other_struct } => variable_struct
                    .fields
                    .push(VariableStructField::NestedStruct {
                        name: other_struct.clone(),
                    }),
                VariableGraphNode::UsedByFragment(fragment_id) => {
                    mapping.push((*fragment_id, variable_struct_index))
                }
                _ => continue,
            }
        }

        variable_struct.fields.sort_by_key(|field| match field {
            VariableStructField::Variable(def) => Some(def.id()),
            VariableStructField::NestedStruct { .. } => None,
        });
    }

    mapping.sort_by_key(|item| item.0);

    VariableStructs {
        structs,
        mapping_sorted: mapping,
    }
}

type VariableGraph<'a> = petgraph::stable_graph::StableDiGraph<VariableGraphNode<'a>, ()>;

/// The variable graph structure
///
/// Generally goes
///
/// Operation
/// -> Variable Struct
///    -> UsedByFragment
///    -> Variable
///    -> Variable
///    -> VariableStruct
///       -> UsedByFragment
///       -> Variable
#[derive(Debug)]
enum VariableGraphNode<'a> {
    /// An operation - the root of the graph
    Operation,
    /// A variable struct that will appear in our output
    VariableStruct { name: String },
    /// A variable inside a variable struct - its name and its type
    Variable(VariableDefinition<'a>),

    /// VariableStructs have an edge to this to indicate that the VariableStruct is used
    /// by this query/inline fragment
    UsedByFragment(FragmentId),
}

fn add_fragment_to_variable_graph<'a>(
    graph: &mut VariableGraph<'a>,
    variables: &HashMap<&'a str, VariableDefinition<'a>>,
    name: String,
    parent: NodeIndex,
    fragment: Fragment<'a>,
) {
    let node = graph.add_node(VariableGraphNode::VariableStruct { name });
    graph.add_edge(parent, node, ());

    let use_node = graph.add_node(VariableGraphNode::UsedByFragment(fragment.id()));
    graph.add_edge(node, use_node, ());

    match fragment {
        Fragment::Inline(fragment) => {
            for variant in fragment.variants() {
                add_fragment_to_variable_graph(
                    graph,
                    variables,
                    format!("{}Variables", fragment.requested_name()),
                    node,
                    variant,
                );
            }
        }
        Fragment::Query(fragment) => {
            for child_selection in fragment.selections() {
                add_selection_to_variable_graph(graph, variables, child_selection, node);
            }
        }
    }
}

fn add_selection_to_variable_graph<'a>(
    graph: &mut VariableGraph<'a>,
    variables: &HashMap<&'a str, VariableDefinition<'a>>,
    selection: graph::Selection<'a>,
    parent: NodeIndex,
) {
    match selection {
        graph::Selection::Field(field) => {
            for arg in field.arguments() {
                add_value_to_variable_graph(graph, variables, arg.value(), parent);
            }

            for directive in field.directives() {
                for argument in directive.arguments() {
                    add_value_to_variable_graph(graph, variables, argument.value(), parent);
                }
            }

            if let Some(fragment) = field.target_fragment() {
                let name = format!("{}Variables", fragment.requested_name());

                add_fragment_to_variable_graph(graph, variables, name, parent, fragment);
            }
        }
        graph::Selection::Spread(spread) => {
            for directive in spread.directives() {
                for argument in directive.arguments() {
                    add_value_to_variable_graph(graph, variables, argument.value(), parent);
                }
            }

            let fragment = spread.target();
            add_fragment_to_variable_graph(
                graph,
                variables,
                format!("{}Variables", fragment.requested_name()),
                parent,
                fragment,
            );
        }
    }
}

fn add_value_to_variable_graph<'a>(
    graph: &mut VariableGraph<'a>,
    variables: &HashMap<&'a str, VariableDefinition<'a>>,
    ty: TypedValue<'a>,
    parent: NodeIndex,
) {
    match ty {
        TypedValue::Variable(variable) => {
            let Some(definition) = variables.get(variable.name()) else {
                panic!("unknown variable: {}", variable.name());
            };
            let node = graph.add_node(VariableGraphNode::Variable(*definition));
            graph.add_edge(parent, node, ());
        }
        TypedValue::List(list) => {
            for item in list.items() {
                add_value_to_variable_graph(graph, variables, item, parent);
            }
        }
        TypedValue::Object(object) => {
            for field in object.fields() {
                add_value_to_variable_graph(graph, variables, field.value(), parent);
            }
        }
        TypedValue::Int(_)
        | TypedValue::Float(_)
        | TypedValue::String(_)
        | TypedValue::Boolean(_)
        | TypedValue::Null(_)
        | TypedValue::Enum(_) => {}
    }
}

fn remove_unused_structs(variable_graph: &mut VariableGraph) {
    loop {
        // UsedByFragment nodes are always present as leaves, so we need to look for any _without_ siblings.
        // Those (and their parents) can always be deleted.
        //
        // We need to do this process iteratively as deleting an empty VariableStruct might
        // cause other VariableStructs to become unused
        let mut to_delete = vec![];

        for leaf in variable_graph.externals(petgraph::Direction::Outgoing) {
            if !matches!(&variable_graph[leaf], VariableGraphNode::UsedByFragment(_)) {
                continue;
            }
            let parent = variable_graph
                .neighbors_directed(leaf, Direction::Incoming)
                .next()
                .expect("UsedByFragment always has a parent");
            let n_siblings = variable_graph.neighbors(parent).count();
            if n_siblings == 1 {
                to_delete.extend([leaf, parent]);
            }
        }

        if to_delete.is_empty() {
            // There are no unused leaves so we can stop iterating
            break;
        }

        for node in to_delete {
            variable_graph.remove_node(node);
        }
    }
}

fn simplify_variable_graph(variable_graph: &mut VariableGraph) {
    // We need to walk the graph DFS post order
    // When visiting a node, we look at each child and see if we're it's only parent
    // If so we merge it into ourselves.  If not, we leave it alone.

    for operation in operation_indices(variable_graph) {
        let mut dfs = DfsPostOrder::new(&*variable_graph, operation);
        while let Some(current_node) = dfs.next(&*variable_graph) {
            let VariableGraphNode::VariableStruct { .. } = &variable_graph[current_node] else {
                continue;
            };
            let children = variable_graph
                .edges_directed(current_node, Direction::Outgoing)
                .filter(|edge| {
                    // We want to preserve any fragment associations this node has
                    !matches!(
                        variable_graph[edge.target()],
                        VariableGraphNode::UsedByFragment(_)
                    )
                })
                .map(|edge| (edge.target(), edge.id()))
                .collect::<Vec<_>>();

            for (child, edge) in children {
                if variable_graph
                    .edges_directed(child, Direction::Incoming)
                    .filter(|edge| {
                        matches!(
                            variable_graph[edge.target()],
                            VariableGraphNode::VariableStruct { .. }
                        )
                    })
                    .count()
                    != 1
                {
                    continue;
                }

                // If there's only one edge incoming to this child we can safely
                // lift it up into the current variable struct
                variable_graph.remove_edge(edge);
                let new_children = variable_graph
                    .edges_directed(child, Direction::Outgoing)
                    .map(|edge| edge.target())
                    .collect::<Vec<_>>();

                for new_child in new_children {
                    variable_graph.add_edge(current_node, new_child, ());
                }

                variable_graph.remove_node(child);
            }
        }
    }
}

fn operation_indices(variable_graph: &VariableGraph) -> Vec<NodeIndex> {
    variable_graph
        .node_references()
        .filter(|node| matches!(node.weight(), VariableGraphNode::Operation))
        .map(|node| node.id())
        .collect()
}
