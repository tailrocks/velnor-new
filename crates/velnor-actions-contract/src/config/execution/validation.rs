use crate::errors::ContractError;

pub(super) fn is_source_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn missing(key: &str) -> ContractError {
    ContractError::config("config.toml", key, "missing")
}
