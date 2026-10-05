use super::prelude::*;
use super::{
    ExecutableId,
    fragment::FragmentDefinition,
    ids::{ExecutableDefinitionId, FragmentDefinitionId, OperationDefinitionId},
    operation::OperationDefinition,
};
#[allow(unused_imports)]
use std::fmt::{self, Write};

pub enum ExecutableDefinitionRecord {
    Operation(OperationDefinitionId),
    Fragment(FragmentDefinitionId),
}

#[derive(Clone, Copy, Debug)]
pub enum ExecutableDefinition<'a> {
    Operation(OperationDefinition<'a>),
    Fragment(FragmentDefinition<'a>),
}

impl<'a> ExecutableDefinition<'a> {
    pub fn is_operation(&self) -> bool {
        matches!(self, ExecutableDefinition::Operation(_))
    }

    pub fn as_operation(self) -> Option<OperationDefinition<'a>> {
        match self {
            Self::Operation(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn is_fragment(&self) -> bool {
        matches!(self, ExecutableDefinition::Fragment(_))
    }

    pub fn as_fragment(self) -> Option<FragmentDefinition<'a>> {
        match self {
            Self::Fragment(inner) => Some(inner),
            _ => None,
        }
    }
}

impl ExecutableId for ExecutableDefinitionId {
    type Reader<'a> = ExecutableDefinition<'a>;
    fn read(self, document: &ExecutableDocument) -> Self::Reader<'_> {
        match document.lookup(self) {
            ExecutableDefinitionRecord::Operation(id) => {
                ExecutableDefinition::Operation(document.read(*id))
            }
            ExecutableDefinitionRecord::Fragment(id) => {
                ExecutableDefinition::Fragment(document.read(*id))
            }
        }
    }
}

impl IdReader for ExecutableDefinition<'_> {
    type Id = ExecutableDefinitionId;
    type Reader<'a> = ExecutableDefinition<'a>;
    fn new(id: Self::Id, document: &'_ ExecutableDocument) -> Self::Reader<'_> {
        document.read(id)
    }
}
