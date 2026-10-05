use std::collections::HashMap;

use cynic_parser::{
    ExecutableDocument, TypeSystemDocument,
    executable::{ExecutableDefinition, OperationDefinition, Selection},
    type_system::{self, Definition},
};

use petgraph::{
    prelude::StableGraph,
    stable_graph::{NodeIndex, StableDiGraph},
};

use super::{Edge, Node};

pub struct GraphBuilder<'a> {
    graph: StableDiGraph<Node, Edge>,
    types: HashMap<String, NodeIndex>,
    type_names: HashMap<NodeIndex, String>,
    named_fragments: HashMap<String, NodeIndex>,
    roots: Option<RootDefinitions>,
    type_system: &'a TypeSystemDocument,
    typename_field: type_system::ids::FieldDefinitionId,
}

impl<'a> GraphBuilder<'a> {
    pub fn new(
        type_system: &'a TypeSystemDocument,
        typename_field: type_system::ids::FieldDefinitionId,
    ) -> Self {
        let mut builder = GraphBuilder {
            graph: StableDiGraph::default(),
            types: HashMap::default(),
            type_names: HashMap::default(),
            named_fragments: HashMap::default(),
            roots: None,
            type_system,
            typename_field,
        };

        ingest_types(&mut builder);
        builder.roots = Some(find_roots(&builder));

        builder
    }

    pub fn finish(self) -> StableGraph<Node, Edge> {
        self.graph
    }
}

impl GraphBuilder<'_> {
    fn root_index(&self, ty: cynic_parser::common::OperationType) -> Option<NodeIndex> {
        let roots = self.roots.as_ref().unwrap();
        match ty {
            cynic_parser::common::OperationType::Query => roots.query,
            cynic_parser::common::OperationType::Mutation => roots.mutation,
            cynic_parser::common::OperationType::Subscription => roots.subscription,
        }
    }

    fn find_field(&self, type_node: NodeIndex, name: &str) -> Option<FieldDetails> {
        let Node::Type(definition_ids) = &self.graph[type_node] else {
            panic!("TypeDefinition should always point at a Node::Type");
        };

        if name == "__typename" {
            let ty_node = *self
                .types
                .get("String")
                .expect("we synthentically add String so this should work");
            return Some(FieldDetails {
                type_node: ty_node,
                definition: self.typename_field,
            });
        }

        for def_id in definition_ids {
            let (Definition::Type(def) | Definition::TypeExtension(def)) =
                self.type_system.read(*def_id)
            else {
                panic!("invalid Type node");
            };
            let field = match def {
                type_system::TypeDefinition::Scalar(_)
                | type_system::TypeDefinition::Union(_)
                | type_system::TypeDefinition::Enum(_)
                | type_system::TypeDefinition::InputObject(_) => None,
                type_system::TypeDefinition::Object(inner) => {
                    inner.fields().find(|field| field.name() == name)
                }
                type_system::TypeDefinition::Interface(inner) => {
                    inner.fields().find(|field| field.name() == name)
                }
            };
            if let Some(definition) = field {
                let type_node = self
                    .types
                    .get(definition.ty().name())
                    .copied()
                    .unwrap_or_else(|| panic!("could not find type {}", definition.ty().name()));

                return Some(FieldDetails {
                    type_node,
                    definition: definition.id(),
                });
            }
        }
        None
    }
}

struct FieldDetails {
    type_node: NodeIndex,
    definition: type_system::ids::FieldDefinitionId,
}

struct RootDefinitions {
    // Technically I think a schema _has_ to have a query but for the sake of querygen lets relax
    // that a little bit and make everything optional
    query: Option<NodeIndex>,
    mutation: Option<NodeIndex>,
    subscription: Option<NodeIndex>,
}

fn ingest_types(builder: &mut GraphBuilder) {
    let definitions = builder.type_system.definitions();

    for (id, definition) in definitions.ids().into_iter().zip(definitions) {
        match definition {
            Definition::Type(definition) | Definition::TypeExtension(definition) => {
                let index = builder
                    .types
                    .entry(definition.name().to_string())
                    .or_insert_with(|| builder.graph.add_node(Node::Type(vec![])));
                builder
                    .type_names
                    .insert(*index, definition.name().to_string());

                let Node::Type(defs) = &mut builder.graph[*index] else {
                    panic!("builder.types malformed somehow");
                };
                defs.push(id)
            }
            _ => {}
        }
    }
}

fn find_roots(builder: &GraphBuilder) -> RootDefinitions {
    let mut query_name = None;
    let mut mutation_name = None;
    let mut subscription_name = None;

    let mut seen_query = false;
    let mut seen_mutation = false;
    let mut seen_subscription = false;
    let mut seen_schema = false;

    for definition in builder.type_system.definitions() {
        match definition {
            Definition::Type(ty) => match ty.name() {
                "Query" => seen_query = true,
                "Mutation" => seen_mutation = true,
                "Subscription" => seen_subscription = true,
                _ => {}
            },
            Definition::Schema(schema) | Definition::SchemaExtension(schema) => {
                seen_schema = true;
                for root_def in schema.root_operations() {
                    match root_def.operation_type() {
                        cynic_parser::common::OperationType::Query => {
                            query_name = Some(root_def.named_type());
                        }
                        cynic_parser::common::OperationType::Mutation => {
                            mutation_name = Some(root_def.named_type());
                        }
                        cynic_parser::common::OperationType::Subscription => {
                            subscription_name = Some(root_def.named_type());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let default_query = (!seen_schema && seen_query).then_some("Query");
    let default_mutation = (!seen_schema && seen_mutation).then_some("Mutation");
    let default_subscription = (!seen_schema && seen_subscription).then_some("Subscription");

    let query = query_name
        .or(default_query)
        .and_then(|name| builder.types.get(name).cloned());
    let mutation = mutation_name
        .or(default_mutation)
        .and_then(|name| builder.types.get(name).cloned());
    let subscription = subscription_name
        .or(default_subscription)
        .and_then(|name| builder.types.get(name).cloned());

    RootDefinitions {
        query,
        mutation,
        subscription,
    }
}

pub fn ingest_executable_doc(builder: &mut GraphBuilder, document: &ExecutableDocument) {
    for definition in document.definitions() {
        if let ExecutableDefinition::Fragment(fragment) = definition {
            ingest_fragment(builder, fragment);
        }
    }

    for definition in document.definitions() {
        if let ExecutableDefinition::Fragment(fragment) = definition {
            ingest_named_fragment_selection_set(builder, fragment);
        }
    }

    for definition in document.definitions() {
        if let ExecutableDefinition::Operation(operation) = definition {
            ingest_operation(builder, operation);
        }
    }
}

fn ingest_fragment(
    builder: &mut GraphBuilder,
    fragment: cynic_parser::executable::FragmentDefinition<'_>,
) {
    let index = builder.graph.add_node(Node::NamedFragment(fragment.id()));
    builder
        .named_fragments
        .insert(fragment.name().to_owned(), index);
}

fn ingest_operation(builder: &mut GraphBuilder, operation: OperationDefinition<'_>) {
    let operation_node = builder.graph.add_node(Node::Operation(operation.id()));
    let root_type = builder
        .root_index(operation.operation_type())
        .unwrap_or_else(|| {
            panic!(
                "could not find root operation for {}",
                operation.operation_type()
            )
        });

    let selection_set_node =
        ingest_selection_set(builder, root_type, operation.selection_set().collect());
    builder
        .graph
        .add_edge(operation_node, selection_set_node, Edge::HasSelectionSet);
}

fn ingest_named_fragment_selection_set(
    builder: &mut GraphBuilder,
    fragment: cynic_parser::executable::FragmentDefinition<'_>,
) {
    let type_node = builder
        .types
        .get(fragment.type_condition())
        .unwrap_or_else(|| panic!("could not find type {}", fragment.type_condition()));

    let selection_set_node =
        ingest_selection_set(builder, *type_node, fragment.selection_set().collect());
    let fragment_node = builder
        .named_fragments
        .get(fragment.name())
        .unwrap_or_else(|| panic!("could not find fragment named {}", fragment.name()));

    builder
        .graph
        .add_edge(*fragment_node, selection_set_node, Edge::HasSelectionSet);
}

fn ingest_selection_set(
    builder: &mut GraphBuilder,
    type_node: NodeIndex,
    selection_set: Vec<Selection<'_>>,
) -> NodeIndex {
    let current_typename = builder
        .type_names
        .get(&type_node)
        .expect("all types should be named");

    let need_query_fragment = selection_set.iter().copied().any(|selection| {
        selection
            .as_field()
            .filter(|field| field.name() != "__typename")
            .is_some()
    });
    let needs_inline_fragments = selection_set
        .iter()
        .copied()
        .any(|selection| match selection {
            Selection::InlineFragment(fragment) => {
                fragment.type_condition().is_some()
                    && fragment.type_condition() != Some(current_typename)
            }
            Selection::FragmentSpread(spread) => {
                spread
                    .fragment()
                    .expect("doc should be validated")
                    .type_condition()
                    != current_typename
            }
            _ => false,
        });

    if need_query_fragment && needs_inline_fragments {
        // Split selections up
        let (query_selections, inline_selections) = selection_set
            .iter()
            .copied()
            .partition::<Vec<_>, _>(|selection| match selection {
                Selection::Field(_) => true,
                Selection::InlineFragment(fragment) => {
                    fragment.type_condition().is_none()
                        || fragment.type_condition() == Some(current_typename)
                }
                Selection::FragmentSpread(spread) => {
                    spread
                        .fragment()
                        .expect("doc should be validated")
                        .type_condition()
                        == current_typename
                }
            });

        assert!(!query_selections.is_empty());
        assert!(!inline_selections.is_empty());

        let query_fragment = ingest_selection_set(builder, type_node, query_selections);
        let inline_fragment = ingest_selection_set(builder, type_node, inline_selections);

        let max_index = builder
            .graph
            .edges(query_fragment)
            .filter_map(|edge| edge.weight().selection_index())
            .max()
            .expect("there should be at least one selection");

        builder.graph.add_edge(
            query_fragment,
            inline_fragment,
            Edge::HasSyntheticSpread {
                index: max_index + 1,
            },
        );

        return query_fragment;
    }

    let this_node = if needs_inline_fragments {
        builder.graph.add_node(Node::InlineFragment)
    } else {
        builder.graph.add_node(Node::QueryFragment)
    };

    builder.graph.add_edge(this_node, type_node, Edge::IsOfType);

    for (index, selection) in selection_set.into_iter().enumerate() {
        match selection {
            Selection::Field(field) => {
                let FieldDetails {
                    type_node: ty_node,
                    definition,
                } = builder
                    .find_field(type_node, field.name())
                    .unwrap_or_else(|| panic!("could not find field {}", field.name()));

                let child_node = if field.selection_set().len() != 0 {
                    ingest_selection_set(builder, ty_node, field.selection_set().collect())
                } else {
                    ingest_scalar_field(builder, ty_node)
                };

                builder.graph.add_edge(
                    this_node,
                    child_node,
                    Edge::HasField {
                        selection: field.id(),
                        definition,
                        index,
                    },
                );
            }
            Selection::InlineFragment(inline_fragment) => {
                let field_type_node = match inline_fragment.type_condition() {
                    Some(name) => {
                        let Some(node) = builder.types.get(name) else {
                            panic!("unknown type used in type condition: {name}");
                        };
                        *node
                    }
                    None => type_node,
                };

                let child_node = ingest_selection_set(
                    builder,
                    field_type_node,
                    inline_fragment.selection_set().collect(),
                );
                builder.graph.add_edge(
                    this_node,
                    child_node,
                    Edge::HasInlineSpread {
                        fragment: inline_fragment.id(),
                        index,
                    },
                );
            }
            Selection::FragmentSpread(spread) => {
                let Some(fragment_node) = builder.named_fragments.get(spread.fragment_name())
                else {
                    panic!("unknown fragment: {}", spread.fragment_name());
                };
                builder.graph.add_edge(
                    this_node,
                    *fragment_node,
                    Edge::HasFragment {
                        fragment: spread.id(),
                        index,
                    },
                );
            }
        }
    }

    this_node
}

fn ingest_scalar_field(builder: &mut GraphBuilder<'_>, type_node: NodeIndex) -> NodeIndex {
    let this_node = builder.graph.add_node(Node::LeafField);
    builder.graph.add_edge(this_node, type_node, Edge::IsOfType);
    this_node
}
