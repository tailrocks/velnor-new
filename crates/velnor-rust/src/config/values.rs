use velnor_model::CheckoutScope;

use crate::error::Reason;

pub(super) fn checkout_scope(sparse: Option<bool>) -> CheckoutScope {
    if sparse.unwrap_or(false) {
        CheckoutScope::Sparse
    } else {
        CheckoutScope::Full
    }
}

pub(super) fn required_check(
    configured: Option<String>,
    default: String,
) -> Result<String, Reason> {
    match configured {
        None => Ok(default),
        Some(name) if !name.trim().is_empty() && !name.chars().any(char::is_control) => Ok(name),
        Some(name) => Err(Reason::InvalidValue {
            key: "required_check",
            value: name,
        }),
    }
}
