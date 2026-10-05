use std::collections::HashSet;

use cynic_parser::executable::VariableDefinition;

use crate::{
    graph::{InputObjectDefinition, TypeDefinition},
    schema::TypeSpec,
};

use crate::graph::GraphReader;
pub struct InputObjects<'a> {
    objects: Vec<InputObjectDefinition<'a>>,
    recursive_objects: HashSet<&'a str>,

    objects_with_lifetime: HashSet<&'a str>,
}

impl<'a> InputObjects<'a> {
    pub fn new(graph: GraphReader<'a>) -> Self {
        let objects = InputObjectIter::from_variables(
            graph
                .operations()
                .flat_map(|operation| operation.variable_definitions()),
            graph,
        )
        .collect();
        let recursive_objects = recursive_objects(graph);
        let objects_with_lifetime = lifetimed_objects(graph);

        InputObjects {
            objects,
            recursive_objects,
            objects_with_lifetime,
        }
    }

    pub fn required_input_types(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.objects
            .iter()
            .flat_map(|object| object.fields().map(|field| field.ty().name()))
    }

    pub fn processed_objects(&self) -> Vec<crate::output::InputObject<'a>> {
        self.objects
            .iter()
            .map(|object| crate::output::InputObject {
                name: object.name().to_string(),
                fields: object
                    .fields()
                    .map(|field| {
                        let inner_type = field.ty().name();
                        let needs_boxed = self.recursive_objects.contains(inner_type);
                        let requires_lifetime = self.objects_with_lifetime.contains(inner_type);
                        crate::output::InputObjectField {
                            schema_field: field,
                            type_spec: TypeSpec::for_input_field(
                                field.type_system_definition(),
                                object.is_one_of().then_some(false),
                                needs_boxed,
                                requires_lifetime,
                            ),
                        }
                    })
                    .collect(),
                schema_name: None,
                is_oneof: object.is_one_of(),
            })
            .collect()
    }
}

fn recursive_objects(graph: GraphReader<'_>) -> HashSet<&str> {
    let mut recursive_objects = HashSet::new();
    for document in graph.operations() {
        for variable in document.variable_definitions() {
            let mut stack = vec![(variable.ty().name(), vec![])];
            let mut seen_objects = HashSet::new();
            while let Some((field_type, mut ancestors)) = stack.pop() {
                let Some(TypeDefinition::InputObject(object)) = graph.type_definition(field_type)
                else {
                    continue;
                };
                if ancestors.contains(&object.name()) {
                    recursive_objects.insert(object.name());
                }
                if seen_objects.contains(object.name()) {
                    continue;
                }
                seen_objects.insert(object.name());
                ancestors.push(object.name());

                for field in object.fields() {
                    stack.push((field.ty().name(), ancestors.clone()))
                }
            }
        }
    }
    recursive_objects
}

fn lifetimed_objects(graph: GraphReader<'_>) -> HashSet<&str> {
    let mut lifetimed_objects = HashSet::new();
    for document in graph.operations() {
        for variable in document.variable_definitions() {
            let mut stack = vec![variable.ty().name()];
            let mut visited = HashSet::new();

            'outer: while !stack.is_empty() {
                if let Some(TypeDefinition::InputObject(object)) =
                    graph.type_definition(stack.last().unwrap())
                {
                    for field in object.fields() {
                        if !visited.contains(&field.ty().name()) {
                            stack.push(field.ty().name());
                            visited.insert(field.ty().name());
                            continue 'outer;
                        }
                    }

                    // If we get here all child field types have been seen.
                    // We need to check whether any child fields need a lifetime...
                    for field in object.fields() {
                        if lifetimed_objects.contains(field.ty().name())
                            || type_requires_lifetime(field.ty().name())
                        {
                            lifetimed_objects.insert(object.name());
                            break;
                        }
                    }
                }

                let visited_node = stack.pop().unwrap();
                visited.insert(visited_node);
            }
        }
    }

    lifetimed_objects
}

pub fn type_requires_lifetime(name: &str) -> bool {
    matches!(name, "String" | "ID")
}

#[derive(Clone)]
struct InputObjectIter<'a> {
    stack: Vec<TypeDefinition<'a>>,
    seen: HashSet<&'a str>,
    graph: GraphReader<'a>,
}

impl<'a> InputObjectIter<'a> {
    fn from_variables(
        variables: impl IntoIterator<Item = VariableDefinition<'a>>,
        graph: GraphReader<'a>,
    ) -> Self {
        InputObjectIter {
            stack: variables
                .into_iter()
                .filter_map(|variable| graph.type_definition(variable.ty().name()))
                .collect(),
            seen: HashSet::new(),
            graph,
        }
    }
}

impl<'a> Iterator for InputObjectIter<'a> {
    type Item = InputObjectDefinition<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.stack.pop()? {
                TypeDefinition::Scalar(_) | TypeDefinition::Enum(_) => continue,
                TypeDefinition::InputObject(input_object)
                    if self.seen.contains(input_object.name()) =>
                {
                    continue;
                }
                TypeDefinition::InputObject(input_object) => {
                    self.seen.insert(input_object.name());
                    for field in input_object.fields().collect::<Vec<_>>().into_iter().rev() {
                        self.stack
                            .extend(self.graph.type_definition(field.ty().name()))
                    }
                    return Some(input_object);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{add_builtins, graph::Graph},
        cynic_parser::{TypeSystemDocument, type_system::ids::FieldDefinitionId},
        std::sync::LazyLock,
    };

    #[test]
    fn deduplicates_input_types_if_same() {
        let (schema, typename_id) = &*GITHUB_SCHEMA;
        let query = cynic_parser::parse_executable_document(
            r#"
              query ($filterOne: IssueFilters!, $filterTwo: IssueFilters!) {
                cynic: repository(owner: "obmarg", name: "cynic") {
                  issues(filterBy: $filterOne) {
                    nodes {
                      title
                    }
                  }
                }
              	kazan: repository(owner: "obmarg", name: "kazan") {
                  issues(filterBy: $filterTwo) {
                    nodes {
                      title
                   }
                  }
                }
              }
            "#,
        )
        .unwrap();

        let graph = Graph::new(&query, schema, *typename_id);
        let reader = graph.reader(&query, schema);

        let input_objects = InputObjects::new(reader);

        assert_eq!(input_objects.objects.len(), 1);
    }

    #[test]
    fn finds_variable_input_types() {
        let (schema, typename_id) = &*GITHUB_SCHEMA;
        let query = cynic_parser::parse_executable_document(
            r#"
              query MyQuery($input: IssueFilters!) {
                cynic: repository(owner: "obmarg", name: "cynic") {
                  issues(filterBy: $input) {
                    nodes {
                      title
                    }
                  }
                }
              	kazan: repository(owner: "obmarg", name: "kazan") {
                  issues(filterBy: $input) {
                    nodes {
                      title
                   }
                  }
                }
              }
            "#,
        )
        .unwrap();

        let graph = Graph::new(&query, schema, *typename_id);
        let reader = graph.reader(&query, schema);

        let input_objects = InputObjects::new(reader);

        assert_eq!(input_objects.objects.len(), 1);
    }

    #[test]
    fn test_extracting_recursive_types() {
        let (schema, typename_id) = &*TEST_CASE_SCHEMA;

        let query = cynic_parser::parse_executable_document(
            r#"
                query MyQuery($input: SelfRecursiveInput!, $input2: RecursiveInputParent!) {
                    recursiveInputField(recursive: $input, recursive2: $input2)
                }
            "#,
        )
        .unwrap();

        let graph = Graph::new(&query, schema, *typename_id);
        let reader = graph.reader(&query, schema);

        let input_objects = InputObjects::new(reader);

        assert_eq!(input_objects.objects.len(), 3);
    }

    static GITHUB_SCHEMA: LazyLock<(TypeSystemDocument, FieldDefinitionId)> = LazyLock::new(|| {
        let schema = cynic_parser::parse_type_system_document(include_str!(
            "../../../schemas/github.graphql"
        ))
        .unwrap();
        add_builtins(schema)
    });

    static TEST_CASE_SCHEMA: LazyLock<(TypeSystemDocument, FieldDefinitionId)> =
        LazyLock::new(|| {
            let schema = cynic_parser::parse_type_system_document(include_str!(
                "../../../schemas/test_cases.graphql"
            ))
            .unwrap();
            add_builtins(schema)
        });
}
