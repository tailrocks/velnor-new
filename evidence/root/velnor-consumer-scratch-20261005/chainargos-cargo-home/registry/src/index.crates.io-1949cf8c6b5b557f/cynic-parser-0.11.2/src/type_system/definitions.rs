use crate::AstLookup;

use super::{
    DefinitionId, Description, Directive, DirectiveDefinition, EnumDefinition,
    InputObjectDefinition, InterfaceDefinition, ObjectDefinition, ScalarDefinition,
    SchemaDefinition, TypeSystemId, UnionDefinition,
    ids::*,
    iter::{IdReader, Iter},
};

#[derive(Clone, Copy)]
pub enum DefinitionRecord {
    Schema(SchemaDefinitionId),
    Scalar(ScalarDefinitionId),
    Object(ObjectDefinitionId),
    Interface(InterfaceDefinitionId),
    Union(UnionDefinitionId),
    Enum(EnumDefinitionId),
    InputObject(InputObjectDefinitionId),
    SchemaExtension(SchemaDefinitionId),
    ScalarExtension(ScalarDefinitionId),
    ObjectExtension(ObjectDefinitionId),
    InterfaceExtension(InterfaceDefinitionId),
    UnionExtension(UnionDefinitionId),
    EnumExtension(EnumDefinitionId),
    InputObjectExtension(InputObjectDefinitionId),
    Directive(DirectiveDefinitionId),
}

#[derive(Clone, Copy)]
pub enum Definition<'a> {
    Schema(SchemaDefinition<'a>),
    SchemaExtension(SchemaDefinition<'a>),
    Type(TypeDefinition<'a>),
    TypeExtension(TypeDefinition<'a>),
    Directive(DirectiveDefinition<'a>),
}

impl Definition<'_> {
    pub fn span(&self) -> crate::Span {
        match self {
            Definition::Schema(def) | Definition::SchemaExtension(def) => def.span(),
            Definition::Type(ty) | Definition::TypeExtension(ty) => ty.span(),
            Definition::Directive(def) => def.span(),
        }
    }
}

impl<'a> Definition<'a> {
    pub fn is_schema(&self) -> bool {
        matches!(self, Definition::Schema(_))
    }

    pub fn as_schema(self) -> Option<SchemaDefinition<'a>> {
        match self {
            Self::Schema(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn is_schema_extension(&self) -> bool {
        matches!(self, Definition::SchemaExtension(_))
    }

    pub fn as_schema_extension(self) -> Option<SchemaDefinition<'a>> {
        match self {
            Self::SchemaExtension(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn is_type(&self) -> bool {
        matches!(self, Definition::Type(_))
    }

    pub fn as_type(self) -> Option<TypeDefinition<'a>> {
        match self {
            Self::Type(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn is_type_extension(&self) -> bool {
        matches!(self, Definition::TypeExtension(_))
    }

    pub fn as_type_extension(self) -> Option<TypeDefinition<'a>> {
        match self {
            Self::TypeExtension(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn is_directive(&self) -> bool {
        matches!(self, Definition::Directive(_))
    }

    pub fn as_directive(self) -> Option<DirectiveDefinition<'a>> {
        match self {
            Self::Directive(inner) => Some(inner),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub enum TypeDefinition<'a> {
    Scalar(ScalarDefinition<'a>),
    Object(ObjectDefinition<'a>),
    Interface(InterfaceDefinition<'a>),
    Union(UnionDefinition<'a>),
    Enum(EnumDefinition<'a>),
    InputObject(InputObjectDefinition<'a>),
}

impl<'a> TypeDefinition<'a> {
    pub fn name(&self) -> &'a str {
        match self {
            TypeDefinition::Scalar(inner) => inner.name(),
            TypeDefinition::Object(inner) => inner.name(),
            TypeDefinition::Interface(inner) => inner.name(),
            TypeDefinition::Union(inner) => inner.name(),
            TypeDefinition::Enum(inner) => inner.name(),
            TypeDefinition::InputObject(inner) => inner.name(),
        }
    }

    pub fn directives(&self) -> Iter<'a, Directive<'a>> {
        match self {
            TypeDefinition::Scalar(inner) => inner.directives(),
            TypeDefinition::Object(inner) => inner.directives(),
            TypeDefinition::Interface(inner) => inner.directives(),
            TypeDefinition::Union(inner) => inner.directives(),
            TypeDefinition::Enum(inner) => inner.directives(),
            TypeDefinition::InputObject(inner) => inner.directives(),
        }
    }

    pub fn span(&self) -> crate::Span {
        match self {
            TypeDefinition::Scalar(inner) => inner.span(),
            TypeDefinition::Object(inner) => inner.span(),
            TypeDefinition::Interface(inner) => inner.span(),
            TypeDefinition::Union(inner) => inner.span(),
            TypeDefinition::Enum(inner) => inner.span(),
            TypeDefinition::InputObject(inner) => inner.span(),
        }
    }

    pub fn description(&self) -> Option<Description<'a>> {
        match self {
            TypeDefinition::Scalar(inner) => inner.description(),
            TypeDefinition::Object(inner) => inner.description(),
            TypeDefinition::Interface(inner) => inner.description(),
            TypeDefinition::Union(inner) => inner.description(),
            TypeDefinition::Enum(inner) => inner.description(),
            TypeDefinition::InputObject(inner) => inner.description(),
        }
    }
}

impl<'a> TypeDefinition<'a> {
    pub fn is_scalar(&self) -> bool {
        matches!(self, TypeDefinition::Scalar(_))
    }

    pub fn as_scalar(&self) -> Option<ScalarDefinition<'a>> {
        match self {
            TypeDefinition::Scalar(inner) => Some(*inner),
            _ => None,
        }
    }

    pub fn is_object(&self) -> bool {
        matches!(self, TypeDefinition::Object(_))
    }

    pub fn as_object(&self) -> Option<ObjectDefinition<'a>> {
        match self {
            TypeDefinition::Object(inner) => Some(*inner),
            _ => None,
        }
    }

    pub fn is_interface(&self) -> bool {
        matches!(self, TypeDefinition::Interface(_))
    }

    pub fn as_interface(&self) -> Option<InterfaceDefinition<'a>> {
        match self {
            TypeDefinition::Interface(inner) => Some(*inner),
            _ => None,
        }
    }

    pub fn is_union(&self) -> bool {
        matches!(self, TypeDefinition::Union(_))
    }

    pub fn as_union(&self) -> Option<UnionDefinition<'a>> {
        match self {
            TypeDefinition::Union(inner) => Some(*inner),
            _ => None,
        }
    }

    pub fn is_enum(&self) -> bool {
        matches!(self, TypeDefinition::Enum(_))
    }

    pub fn as_enum(&self) -> Option<EnumDefinition<'a>> {
        match self {
            TypeDefinition::Enum(inner) => Some(*inner),
            _ => None,
        }
    }

    pub fn is_input_object(&self) -> bool {
        matches!(self, TypeDefinition::InputObject(_))
    }

    pub fn as_input_object(&self) -> Option<InputObjectDefinition<'a>> {
        match self {
            TypeDefinition::InputObject(inner) => Some(*inner),
            _ => None,
        }
    }
}

impl TypeSystemId for DefinitionId {
    type Reader<'a> = Definition<'a>;

    fn read(self, document: &super::TypeSystemDocument) -> Self::Reader<'_> {
        match document.lookup(self) {
            DefinitionRecord::Schema(id) => Definition::Schema(document.read(*id)),
            DefinitionRecord::Scalar(id) => {
                Definition::Type(TypeDefinition::Scalar(document.read(*id)))
            }
            DefinitionRecord::Object(id) => {
                Definition::Type(TypeDefinition::Object(document.read(*id)))
            }
            DefinitionRecord::Interface(id) => {
                Definition::Type(TypeDefinition::Interface(document.read(*id)))
            }
            DefinitionRecord::Union(id) => {
                Definition::Type(TypeDefinition::Union(document.read(*id)))
            }
            DefinitionRecord::Enum(id) => {
                Definition::Type(TypeDefinition::Enum(document.read(*id)))
            }
            DefinitionRecord::InputObject(id) => {
                Definition::Type(TypeDefinition::InputObject(document.read(*id)))
            }
            DefinitionRecord::SchemaExtension(id) => {
                Definition::SchemaExtension(document.read(*id))
            }
            DefinitionRecord::ScalarExtension(id) => {
                Definition::TypeExtension(TypeDefinition::Scalar(document.read(*id)))
            }
            DefinitionRecord::ObjectExtension(id) => {
                Definition::TypeExtension(TypeDefinition::Object(document.read(*id)))
            }
            DefinitionRecord::InterfaceExtension(id) => {
                Definition::TypeExtension(TypeDefinition::Interface(document.read(*id)))
            }
            DefinitionRecord::UnionExtension(id) => {
                Definition::TypeExtension(TypeDefinition::Union(document.read(*id)))
            }
            DefinitionRecord::EnumExtension(id) => {
                Definition::TypeExtension(TypeDefinition::Enum(document.read(*id)))
            }
            DefinitionRecord::InputObjectExtension(id) => {
                Definition::TypeExtension(TypeDefinition::InputObject(document.read(*id)))
            }
            DefinitionRecord::Directive(id) => Definition::Directive(document.read(*id)),
        }
    }
}

impl IdReader for Definition<'_> {
    type Id = DefinitionId;
    type Reader<'a> = Definition<'a>;

    fn new(id: Self::Id, document: &'_ super::TypeSystemDocument) -> Self::Reader<'_> {
        document.read(id)
    }
}
