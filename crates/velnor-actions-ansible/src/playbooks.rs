//! Top-level playbook discovery for `ansible-playbook --syntax-check`.

use crate::paths::is_under;

/// True when `text` is a playbook: a YAML list whose first item is a play
/// (`hosts:`) or an `import_playbook`.
#[must_use]
pub fn is_playbook(text: &str) -> bool {
    let mut lines = text
        .lines()
        .map(|line| line.split(" #").next().unwrap_or("").trim_end())
        .filter(|line| {
            !line.trim().is_empty() && !line.trim_start().starts_with('#') && *line != "---"
        });
    let Some(first) = lines.next() else {
        return false;
    };
    let Some(item) = first.strip_prefix("- ") else {
        return false;
    };
    let is_play = |text: &str| {
        let text = text.trim_start();
        text.starts_with("hosts:") || text.starts_with("import_playbook:")
    };
    is_play(item) || lines.take_while(|l| l.starts_with(' ')).any(is_play)
}

/// Sorted top-level playbooks under `root`: `(path, text)` pairs of `.yml`
/// and `.yaml` files directly in `root` whose text is a playbook.
#[must_use]
pub fn top_level_playbooks(root: &str, files: &[(&str, &str)]) -> Vec<String> {
    let root = if root == "." { "" } else { root };
    let mut found: Vec<String> = files
        .iter()
        .filter(|(path, _)| {
            let Some(name) = (if root.is_empty() {
                Some(*path)
            } else {
                path.strip_prefix(root).and_then(|r| r.strip_prefix('/'))
            }) else {
                return false;
            };
            !name.contains('/')
                && std::path::Path::new(name).extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("yml") || e.eq_ignore_ascii_case("yaml")
                })
        })
        .filter(|(path, text)| is_under(path, root) && is_playbook(text))
        .map(|(path, _)| (*path).to_owned())
        .collect();
    found.sort();
    found
}
