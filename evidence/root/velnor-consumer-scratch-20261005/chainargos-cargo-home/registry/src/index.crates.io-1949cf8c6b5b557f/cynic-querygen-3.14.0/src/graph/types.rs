use cynic_parser::common::{TypeWrappers, TypeWrappersIter, WrappingType};

use super::TypeDefinition;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Type<'a> {
    pub(super) definition: super::TypeDefinition<'a>,
    pub(super) wrappers: TypeWrappers,
}

impl std::fmt::Debug for Type<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for wrapper in self.wrappers.iter() {
            if wrapper == WrappingType::List {
                write!(f, "[")?;
            }
        }
        write!(f, "{}", self.definition.name())?;

        for wrapper in self.wrappers.iter().rev() {
            match wrapper {
                WrappingType::NonNull => write!(f, "!")?,
                WrappingType::List => write!(f, "]")?,
            }
        }
        Ok(())
    }
}

impl<'a> Type<'a> {
    pub fn name(&self) -> &'a str {
        self.definition().name()
    }

    pub fn definition(&self) -> TypeDefinition<'a> {
        self.definition
    }

    /// Iterator over wrapper types from outermost to innermost
    pub fn wrappers(&self) -> TypeWrappersIter {
        self.wrappers.iter()
    }

    pub(crate) fn with_wrappers(&self, wrappers: TypeWrappers) -> Type<'a> {
        Type { wrappers, ..*self }
    }
}
