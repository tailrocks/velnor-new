use std::path::Path;

use toml::Value as TomlValue;

use super::context::py_repr;

pub(super) fn manifest_subject(relative: &Path) -> String {
    if relative.as_os_str().is_empty() {
        "Cargo.toml".to_owned()
    } else {
        path_text(&relative.join("Cargo.toml"))
    }
}

pub(super) fn path_text(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

pub(super) fn toml_repr(value: &TomlValue) -> String {
    serde_json::to_value(value).map_or_else(|_| value.to_string(), |value| py_repr(Some(&value)))
}
