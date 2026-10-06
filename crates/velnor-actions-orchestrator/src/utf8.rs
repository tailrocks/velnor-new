//! UTF-8 converters for fixed argv and env pairs.

use std::collections::BTreeMap;
use std::ffi::OsString;

/// Convert fixed argv to UTF-8 strings.
pub(crate) fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(argv.len());
    for arg in argv {
        match arg.into_string() {
            Ok(text) => out.push(text),
            Err(_) => return Err("non_utf8_argv".to_owned()),
        }
    }
    Ok(out)
}

/// Convert fixed env pairs to UTF-8 strings.
pub(crate) fn strings_of_env(
    env: &[(OsString, OsString)],
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (key, value) in env {
        let (Some(key), Some(value)) = (key.to_str(), value.to_str()) else {
            return Err("non_utf8_env".to_owned());
        };
        out.insert(key.to_owned(), value.to_owned());
    }
    Ok(out)
}
