#[must_use]
pub fn normalize(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for segment in rel.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

#[must_use]
pub fn under_root(root: &str, rel: &str) -> String {
    let root = if root == "." { "" } else { root };
    match (root.is_empty(), rel.is_empty()) {
        (true, _) => rel.to_owned(),
        (false, true) => root.to_owned(),
        (false, false) => format!("{root}/{rel}"),
    }
}

#[must_use]
pub fn is_under(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[must_use]
pub fn is_tsconfig(name: &str) -> bool {
    name.starts_with("tsconfig")
        && std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}
