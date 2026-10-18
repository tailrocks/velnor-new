use std::collections::HashMap;

use syn::{Path, QSelf, Type};

pub(super) fn resolve_path(
    qself: &QSelf,
    path: &Path,
    aliases: &HashMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    let Type::Path(type_path) = &*qself.ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }
    let mut canonical = super::resolve_path(&type_path.path, aliases);
    canonical.extend(
        path.segments
            .iter()
            .skip(qself.position)
            .map(|segment| segment.ident.to_string()),
    );
    Some(canonical)
}

pub(super) fn is_file_operation(path: &Path) -> bool {
    path.segments.iter().any(|segment| {
        matches!(
            segment.ident.to_string().as_str(),
            "open"
                | "openat"
                | "openat2"
                | "create"
                | "options"
                | "OpenOptions"
                | "write"
                | "create_dir"
                | "create_dir_all"
                | "rename"
                | "remove_file"
                | "remove_dir"
                | "remove_dir_all"
                | "copy"
                | "hard_link"
                | "set_permissions"
        )
    })
}
