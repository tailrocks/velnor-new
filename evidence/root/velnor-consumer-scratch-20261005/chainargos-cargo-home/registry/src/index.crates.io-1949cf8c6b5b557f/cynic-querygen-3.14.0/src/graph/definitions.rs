use cynic_parser::type_system::{self, Definition, EnumValueDefinition};

use super::{Directive, GraphReader, Node, NodeRef, Type};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TypeDefinition<'a> {
    Object(ObjectDefinition<'a>),
    InputObject(InputObjectDefinition<'a>),
    Enum(EnumDefinition<'a>),
    Interface(InterfaceDefinition<'a>),
    Union(UnionDefinition<'a>),
    Scalar(ScalarDefinition<'a>),
}

impl<'a> TypeDefinition<'a> {
    pub(super) fn new(node: NodeRef<'a>) -> Self {
        let ids = defs_from_node(node);
        let id = ids.first().expect("this list should never be empty");

        let def = type_def_from_id(node.reader, *id);

        match def {
            type_system::TypeDefinition::Scalar(_) => Self::Scalar(ScalarDefinition(node)),
            type_system::TypeDefinition::Object(_) => Self::Object(ObjectDefinition(node)),
            type_system::TypeDefinition::Interface(_) => Self::Interface(InterfaceDefinition(node)),
            type_system::TypeDefinition::Union(_) => Self::Union(UnionDefinition(node)),
            type_system::TypeDefinition::Enum(_) => Self::Enum(EnumDefinition(node)),
            type_system::TypeDefinition::InputObject(_) => {
                Self::InputObject(InputObjectDefinition(node))
            }
        }
    }

    pub fn name(&self) -> &'a str {
        self.defs().next().expect("must be one def").name()
    }

    fn defs(self) -> impl Iterator<Item = type_system::TypeDefinition<'a>> + 'a {
        let node_ref = match self {
            TypeDefinition::Object(inner) => inner.0,
            TypeDefinition::InputObject(inner) => inner.0,
            TypeDefinition::Enum(inner) => inner.0,
            TypeDefinition::Interface(inner) => inner.0,
            TypeDefinition::Union(inner) => inner.0,
            TypeDefinition::Scalar(inner) => inner.0,
        };
        defs_from_node(node_ref)
            .iter()
            .map(move |id| node_ref.reader.type_system.read(*id))
            .filter_map(|def| def.as_type().or_else(|| def.as_type_extension()))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ObjectDefinition<'a>(NodeRef<'a>);

impl<'a> ObjectDefinition<'a> {
    #[expect(unused)]
    pub fn name(&self) -> &'a str {
        TypeDefinition::Object(*self).name()
    }

    #[expect(unused)]
    pub fn field(&self, name: &str) -> Option<FieldDefinition<'a>> {
        let definition_ids = defs_from_node(self.0);

        for def_id in definition_ids {
            let def = type_def_from_id(self.0.reader, *def_id)
                .as_object()
                .expect("invalid ObjectDefinition");

            if let Some(field) = def.fields().find(|field| field.name() == name) {
                return Some(FieldDefinition {
                    field,
                    container: FieldContainer::Object(*self),
                });
            }
        }
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct InputObjectDefinition<'a>(NodeRef<'a>);

impl<'a> InputObjectDefinition<'a> {
    pub fn name(&self) -> &'a str {
        TypeDefinition::InputObject(*self).name()
    }

    pub fn fields(&self) -> impl Iterator<Item = InputValueDefinition<'a>> {
        self.defs()
            .flat_map(|def| def.fields())
            .map(|value| InputValueDefinition {
                value,
                container: InputValueContainer::InputObject(*self),
            })
    }

    pub fn field(&self, name: &str) -> Option<InputValueDefinition<'a>> {
        let definition_ids = defs_from_node(self.0);

        for def_id in definition_ids {
            let def = type_def_from_id(self.0.reader, *def_id)
                .as_input_object()
                .expect("invalid InputObjectDefinition");

            if let Some(value) = def.fields().find(|value| value.name() == name) {
                return Some(InputValueDefinition {
                    value,
                    container: InputValueContainer::InputObject(*self),
                });
            }
        }
        None
    }

    pub fn is_one_of(&self) -> bool {
        // This may accept some invalid inputs (in that only the _actual_ definition can define
        // an object as oneOf - but I'm fine with that for now)
        self.defs().any(|def| def.is_one_of())
    }

    fn defs(&self) -> impl Iterator<Item = type_system::InputObjectDefinition<'a>> + 'a {
        let obj = TypeDefinition::InputObject(*self);
        obj.defs().filter_map(move |def| def.as_input_object())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EnumDefinition<'a>(NodeRef<'a>);

impl<'a> EnumDefinition<'a> {
    pub fn name(&self) -> &'a str {
        TypeDefinition::Enum(*self).name()
    }

    pub fn values(&self) -> impl Iterator<Item = EnumValueDefinition<'a>> {
        TypeDefinition::Enum(*self)
            .defs()
            .filter_map(|type_def| type_def.as_enum())
            .flat_map(|enum_def| enum_def.values())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct InterfaceDefinition<'a>(NodeRef<'a>);

impl<'a> InterfaceDefinition<'a> {
    #[expect(unused)]
    pub fn name(&self) -> &'a str {
        TypeDefinition::Interface(*self).name()
    }

    #[expect(unused)]
    pub fn field(&self, name: &str) -> Option<FieldDefinition<'a>> {
        let definition_ids = defs_from_node(self.0);

        for def_id in definition_ids {
            let def = type_def_from_id(self.0.reader, *def_id)
                .as_interface()
                .expect("invalid InterfaceDefinition");

            if let Some(field) = def.fields().find(|field| field.name() == name) {
                return Some(FieldDefinition {
                    field,
                    container: FieldContainer::Interface(*self),
                });
            }
        }
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UnionDefinition<'a>(NodeRef<'a>);

impl<'a> UnionDefinition<'a> {
    #[expect(unused)]
    pub fn name(&self) -> &'a str {
        TypeDefinition::Union(*self).name()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ScalarDefinition<'a>(NodeRef<'a>);

impl<'a> ScalarDefinition<'a> {
    pub fn name(&self) -> &'a str {
        TypeDefinition::Scalar(*self).name()
    }
}

#[derive(Clone, Copy)]
pub struct FieldDefinition<'a> {
    pub(super) field: type_system::FieldDefinition<'a>,
    pub(super) container: FieldContainer<'a>,
}

impl<'a> FieldDefinition<'a> {
    pub fn argument_definitions(&self) -> impl Iterator<Item = InputValueDefinition<'a>> {
        self.field.arguments().map(|value| InputValueDefinition {
            value,
            container: InputValueContainer::Field(*self),
        })
    }

    pub fn ty(&self) -> Type<'a> {
        Type {
            definition: self
                .container
                .reader()
                .type_definition(self.field.ty().name())
                .unwrap_or_else(|| panic!("could not find type {}", self.field.ty().name())),
            wrappers: self.field.ty().wrappers().collect(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldContainer<'a> {
    Object(ObjectDefinition<'a>),
    Interface(InterfaceDefinition<'a>),
    // Unions mostly don't have fields but they do still have __typename
    Union(UnionDefinition<'a>),
}

impl<'a> FieldContainer<'a> {
    fn reader(&self) -> GraphReader<'a> {
        match self {
            FieldContainer::Object(inner) => inner.0.reader,
            FieldContainer::Interface(inner) => inner.0.reader,
            FieldContainer::Union(inner) => inner.0.reader,
        }
    }
}

#[derive(Clone, Copy)]
pub struct InputValueDefinition<'a> {
    value: type_system::InputValueDefinition<'a>,
    container: InputValueContainer<'a>,
}

impl<'a> InputValueDefinition<'a> {
    pub(super) fn directive_argument(
        value: type_system::InputValueDefinition<'a>,
        directive: Directive<'a>,
    ) -> Self {
        Self {
            value,
            container: InputValueContainer::Directive(directive),
        }
    }
}

#[derive(Clone, Copy)]
pub enum InputValueContainer<'a> {
    InputObject(InputObjectDefinition<'a>),
    Field(FieldDefinition<'a>),
    Directive(Directive<'a>),
}

impl<'a> InputValueContainer<'a> {
    fn reader(&self) -> GraphReader<'a> {
        match self {
            InputValueContainer::InputObject(inner) => inner.0.reader,
            InputValueContainer::Field(inner) => inner.container.reader(),
            InputValueContainer::Directive(inner) => inner.reader,
        }
    }
}

impl<'a> InputValueDefinition<'a> {
    pub fn name(&self) -> &'a str {
        self.value.name()
    }

    pub fn ty(&self) -> Type<'a> {
        Type {
            definition: self
                .container
                .reader()
                .type_definition(self.value.ty().name())
                .unwrap_or_else(|| panic!("could not find type {}", self.value.ty().name())),
            wrappers: self.value.ty().wrappers().collect(),
        }
    }

    pub fn type_system_definition(&self) -> type_system::InputValueDefinition<'a> {
        self.value
    }
}

fn defs_from_node(node: NodeRef<'_>) -> &Vec<type_system::ids::DefinitionId> {
    let Node::Type(ids) = node.weight() else {
        panic!("TypeDefinition should always point at a Node::Type");
    };
    ids
}

fn type_def_from_id(
    reader: GraphReader<'_>,
    id: type_system::ids::DefinitionId,
) -> type_system::TypeDefinition<'_> {
    let def = match reader.type_system.read(id) {
        Definition::Type(def) | Definition::TypeExtension(def) => def,
        _ => {
            panic!("invalid TypeDefinition");
        }
    };
    def
}
