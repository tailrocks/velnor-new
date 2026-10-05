//! REUSE license metadata obligations.

/// Propose the fixed metadata compliance validation command.
#[must_use]
pub fn lint() -> Vec<String> {
    vec!["reuse".to_owned(), "lint".to_owned()]
}
