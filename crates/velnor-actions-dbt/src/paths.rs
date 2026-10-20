//! Repository-relative path arithmetic shared by the dbt modules.

/// Join `rel` onto `base` (both repository-relative, `/`-separated), folding
/// `.` and `..`. `None` when the result leaves the repository root.
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

/// True when `rel` stays inside the directory it is relative to.
#[must_use]
pub fn stays_inside(rel: &str) -> bool {
    !rel.starts_with('/') && normalize("", rel).is_some()
}

/// Repository path of `rel` under `dir` (`.` and `""` name the repository root).
#[must_use]
pub fn under(dir: &str, rel: &str) -> String {
    let dir = if dir == "." { "" } else { dir };
    match (dir.is_empty(), rel.is_empty()) {
        (true, _) => rel.to_owned(),
        (false, true) => dir.to_owned(),
        (false, false) => format!("{dir}/{rel}"),
    }
}

/// True when `path` equals `dir` or lies under it (`""` and `.` are every path).
#[must_use]
pub fn is_under(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || dir == "."
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}
