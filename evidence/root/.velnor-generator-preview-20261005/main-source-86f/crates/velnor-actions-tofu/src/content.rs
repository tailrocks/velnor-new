//! E4 content signals: `required_version` + legacy refs + terraform-only pins.
//!
//! Signals derive from bounded structural parses (never raw text
//! scans, so native `terraform` keywords and labels cannot fire).
//! Parse failures propagate: inventory surfaces them as malformed
//! units, table-less evidence skips the file (no table, no claim).

use crate::effective::Dialect;
use crate::parser::{FileModel, ParseError, parse_json, parse_native};
use crate::version::is_terraform_only;

/// Content signals of one config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSignals {
    /// `required_version` literals in encounter order.
    pub required_versions: Vec<String>,
    /// Whether a legacy `terraform` token appears in string literals.
    pub has_legacy_ref: bool,
    /// Whether any `required_version` admits no `OpenTofu` release.
    pub terraform_only: bool,
}

/// Signals for one file's text in `dialect`.
///
/// # Errors
///
/// Returns [`ParseError`] for malformed input.
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
