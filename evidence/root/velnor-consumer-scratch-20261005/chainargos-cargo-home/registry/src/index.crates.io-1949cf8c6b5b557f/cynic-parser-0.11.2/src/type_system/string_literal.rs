use crate::{
    AstLookup,
    common::StringLiteral,
    type_system::{TypeSystemId, ids::StringLiteralId},
};

impl TypeSystemId for StringLiteralId {
    type Reader<'a> = StringLiteral<'a>;

    fn read(self, document: &super::TypeSystemDocument) -> Self::Reader<'_> {
        match self {
            StringLiteralId::String(id) => StringLiteral::new_string(document.lookup(id)),
            StringLiteralId::Block(id) => StringLiteral::new_block(document.lookup(id)),
        }
    }
}
