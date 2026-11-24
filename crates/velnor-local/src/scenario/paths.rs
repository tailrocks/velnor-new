use std::path::{Component, Path};

#[must_use]
pub fn with_path(list: Option<&str>, path: &str) -> String {
    match list {
        Some(list) if !list.is_empty() => format!("{list}:{path}"),
        _ => path.to_owned(),
    }
}

#[must_use]
pub fn link_inside(link: &Path, target: &Path) -> bool {
    let mut depth = link.components().count().saturating_sub(1);
    for part in target.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    true
}

#[must_use]
pub fn overlapping(state: &Path, checkout: &Path) -> bool {
    state.starts_with(checkout) || checkout.starts_with(state)
}

#[must_use]
pub fn inside(root: &Path, dir: &Path) -> bool {
    dir.starts_with(root)
}
