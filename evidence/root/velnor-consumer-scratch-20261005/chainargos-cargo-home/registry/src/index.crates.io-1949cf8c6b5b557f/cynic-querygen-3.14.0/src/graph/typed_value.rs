use cynic_parser::{
    Value,
    common::TypeWrappers,
    values::{BooleanValue, EnumValue, FloatValue, IntValue, List, NullValue, Object, StringValue},
};

use super::{Type, TypeDefinition};

#[derive(Clone, Copy, PartialEq)]
pub enum TypedValue<'a> {
    Variable(TypedVariableValue<'a>),
    Int(TypedIntValue<'a>),
    Float(TypedFloatValue<'a>),
    String(TypedStringValue<'a>),
    Boolean(TypedBooleanValue<'a>),
    Null(TypedNullValue<'a>),
    Enum(TypedEnumValue<'a>),
    List(TypedList<'a>),
    Object(TypedObject<'a>),
}

impl std::fmt::Debug for TypedValue<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Variable(_) => f.debug_tuple("Variable").finish_non_exhaustive(),
            Self::Int(_) => f.debug_tuple("Int").finish_non_exhaustive(),
            Self::Float(_) => f.debug_tuple("Float").finish_non_exhaustive(),
            Self::String(_) => f.debug_tuple("String").finish_non_exhaustive(),
            Self::Boolean(_) => f.debug_tuple("Boolean").finish_non_exhaustive(),
            Self::Null(_) => f.debug_tuple("Null").finish_non_exhaustive(),
            Self::Enum(_) => f.debug_tuple("Enum").finish_non_exhaustive(),
            Self::List(_) => f.debug_tuple("List").finish_non_exhaustive(),
            Self::Object(_) => f.debug_tuple("Object").finish_non_exhaustive(),
        }
    }
}

impl<'a> TypedValue<'a> {
    pub fn new(value: Value<'a>, ty: Type<'a>) -> Self {
        match value {
            Value::Variable(value) => TypedValue::Variable(TypedVariableValue { value, ty }),
            Value::Int(value) => TypedValue::Int(TypedIntValue { value, ty }),
            Value::Float(value) => TypedValue::Float(TypedFloatValue { value, ty }),
            Value::String(value) => TypedValue::String(TypedStringValue { value, ty }),
            Value::Boolean(value) => TypedValue::Boolean(TypedBooleanValue { value, ty }),
            Value::Null(value) => TypedValue::Null(TypedNullValue { value, ty }),
            Value::Enum(value) => TypedValue::Enum(TypedEnumValue { value, ty }),
            Value::List(value) => TypedValue::List(TypedList { value, ty }),
            Value::Object(value) => TypedValue::Object(TypedObject { value, ty }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedVariableValue<'a> {
    value: cynic_parser::values::VariableValue<'a>,
    ty: Type<'a>,
}

impl<'a> TypedVariableValue<'a> {
    pub fn name(&self) -> &'a str {
        self.value.name()
    }

    #[expect(unused)]
    pub fn ty(&self) -> Type<'a> {
        self.ty
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TypedIntValue<'a> {
    pub value: IntValue<'a>,
    pub ty: Type<'a>,
}

#[derive(Clone, Copy, PartialEq)]
pub struct TypedFloatValue<'a> {
    pub value: FloatValue<'a>,
    pub ty: Type<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TypedStringValue<'a> {
    pub value: StringValue<'a>,
    pub ty: Type<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TypedBooleanValue<'a> {
    pub value: BooleanValue<'a>,
    pub ty: Type<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TypedNullValue<'a> {
    pub value: NullValue<'a>,
    pub ty: Type<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TypedEnumValue<'a> {
    pub value: EnumValue<'a>,
    pub ty: Type<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TypedList<'a> {
    pub value: List<'a>,
    pub ty: Type<'a>,
}

impl<'a> TypedList<'a> {
    pub fn items(&self) -> impl Iterator<Item = TypedValue<'a>> {
        let mut wrappers = self.ty.wrappers().peekable();
        if wrappers.peek() == Some(&cynic_parser::common::WrappingType::NonNull) {
            wrappers.next();
        }
        if wrappers.peek() == Some(&cynic_parser::common::WrappingType::List) {
            wrappers.next();
        }
        let new_wrappers = wrappers.collect::<TypeWrappers>();
        self.value
            .items()
            .map(move |value| TypedValue::new(value, self.ty.with_wrappers(new_wrappers)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TypedObject<'a> {
    pub value: Object<'a>,
    pub ty: Type<'a>,
}

impl<'a> TypedObject<'a> {
    pub fn fields(&self) -> impl Iterator<Item = TypedObjectField<'a>> {
        let TypeDefinition::InputObject(object) = self.ty.definition() else {
            panic!("malformed TypedObject")
        };
        self.value.fields().map(move |field| {
            let Some(field_def) = object.field(field.name()) else {
                panic!("unknown field: {}.{}", object.name(), field.name());
            };
            TypedObjectField {
                name: field.name(),
                value: TypedValue::new(field.value(), field_def.ty()),
            }
        })
    }
}

#[derive(Clone, Copy)]
pub struct TypedObjectField<'a> {
    pub name: &'a str,
    pub value: TypedValue<'a>,
}

impl<'a> TypedObjectField<'a> {
    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn value(&self) -> TypedValue<'a> {
        self.value
    }
}
