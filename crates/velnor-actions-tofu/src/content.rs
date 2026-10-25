use crate::effective::Dialect;
use crate::parser::{FileModel, ParseError, parse_json, parse_native};
use crate::version::is_terraform_only;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSignals {
    pub required_versions: Vec<String>,
    pub has_legacy_ref: bool,
    pub terraform_only: bool,
}

pub fn signals_for(text: &str, dialect: Dialect) -> Result<ContentSignals, ParseError> {
    let model: FileModel = match dialect {
        Dialect::Native => parse_native(text)?,
        Dialect::Json => parse_json(text)?,
    };
    Ok(ContentSignals {
        terraform_only: model
            .required_versions
            .iter()
            .any(|constraint| is_terraform_only(constraint)),
        required_versions: model.required_versions,
        has_legacy_ref: model.has_legacy_ref,
    })
}
