use super::{reverse_units, source_files, source_units};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::hard_link;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::symlink;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::net::UnixListener;

const FIXTURES: &[&str] = &[
    "crates/velnor-actions-mise/tests/impl_mise_git_owned_config_fixture.rs",
    "crates/velnor-actions-mise/tests/impl_mise_git_index_fixture.rs",
    "crates/test_support/git_fixture.rs",
];

static TREE_ID: AtomicU64 = AtomicU64::new(0);

struct Tree {
    root: PathBuf,
}

impl Drop for Tree {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "source inventory fixture cleanup failed: {}: {error}",
                    self.root.display()
                );
            }
        }
    }
}

fn tree(label: &str) -> Result<Tree, String> {
    let id = TREE_ID.fetch_add(1, Ordering::Relaxed);
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .map_err(|error| format!("canonicalize fixture parent: {error}"))?;
    let root = temp_root.join(format!(
        "velnor-source-inventory-{}-{id}-{label}",
        std::process::id()
    ));
    fs::create_dir_all(&root).map_err(|error| format!("create fixture root: {error}"))?;
    Ok(Tree { root })
}

fn repo(tree: &Tree, lib: &str) -> Result<PathBuf, String> {
    repo_at(&tree.root, lib)
}

fn repo_at(root: &Path, lib: &str) -> Result<PathBuf, String> {
    let src = root.join("crates/velnor-actions-mise/src");
    fs::create_dir_all(src.join("nested/deeper")).map_err(|error| error.to_string())?;
    fs::write(src.join("lib.rs"), lib).map_err(|error| error.to_string())?;
    fs::write(src.join("nested/child.rs"), "pub(crate) fn child() {}")
        .map_err(|error| error.to_string())?;
    fs::write(src.join("nested/deeper/leaf.txt"), "ignored").map_err(|error| error.to_string())?;
    for relative in FIXTURES {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let source = if relative.ends_with("git_owned_config_fixture.rs") {
            "#[cfg(test)] #[path=\"../../test_support/git_fixture.rs\"] mod git_fixture;"
        } else if relative.ends_with("git_index_fixture.rs") {
            "pub(crate) fn fixture() {}"
        } else {
            "pub(crate) fn setup() {}"
        };
        fs::write(path, source).map_err(|error| error.to_string())?;
    }
    Ok(src)
}

fn graph_errors(units: &[(String, String)]) -> Vec<String> {
    let mut errors = Vec::new();
    super::super::policy::check_source_graph(units, &mut errors);
    errors
}

#[test]
fn source_units_recurses_and_adds_exact_fixture_closure() -> Result<(), String> {
    let fixture = tree("recursive")?;
    let src = repo(&fixture, "pub fn root() {}")?;
    let units = source_units(&src)?;
    let names: Vec<_> = units.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names.len(), 5);
    assert!(names.contains(&"crates/velnor-actions-mise/src/lib.rs"));
    assert!(names.contains(&"crates/velnor-actions-mise/src/nested/child.rs"));
    for path in FIXTURES {
        assert!(names.contains(path), "missing {path}");
    }
    assert!(!names.iter().any(|name| name.ends_with("leaf.txt")));
    Ok(())
}

#[test]
fn loaded_tuple_inventory_resolves_module_targets() -> Result<(), String> {
    let fixture = tree("graph")?;
    let source =
        "#[cfg(test)] #[path=\"../tests/impl_mise_git_owned_config_fixture.rs\"] mod fixture;";
    let src = repo(&fixture, source)?;
    let units = source_units(&src)?;
    let errors = graph_errors(&units);
    assert!(errors.is_empty(), "{errors:?}");
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn reverse_units_includes_ordinary_test_text() -> Result<(), String> {
    let fixture = tree("reverse")?;
    let src = repo(&fixture, "pub fn root() {}")?;
    let ordinary = fixture
        .root
        .join("crates/velnor-actions-mise/tests/impl_mise_git_index.rs");
    fs::write(&ordinary, "#[test] fn ordinary() {}").map_err(|error| error.to_string())?;
    let units = reverse_units(&src)?;
    let names: Vec<_> = units.iter().map(|(name, _)| name.as_str()).collect();
    assert!(names.contains(&"crates/velnor-actions-mise/tests/impl_mise_git_index.rs"));
    assert!(names.contains(&"crates/test_support/git_fixture.rs"));
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn reverse_units_rejects_symlinked_test_root() -> Result<(), String> {
    let fixture = tree("reverse-link")?;
    let src = repo(&fixture, "pub fn root() {}")?;
    let tests = fixture.root.join("crates/velnor-actions-mise/tests");
    let physical = fixture.root.join("physical-tests");
    fs::rename(&tests, &physical).map_err(|error| error.to_string())?;
    symlink(&physical, &tests).map_err(|error| error.to_string())?;
    assert!(reverse_units(&src).is_err());
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn oversized_source_is_refused_before_snapshot_completion() -> Result<(), String> {
    let fixture = tree("size-limit")?;
    let src = repo(&fixture, "pub fn root() {}")?;
    let oversized = vec![b'x'; 16 * 1024 * 1024 + 1];
    fs::write(src.join("oversized.rs"), oversized).map_err(|error| error.to_string())?;
    let error = match source_units(&src) {
        Ok(_) => return Err("oversized source was accepted".to_owned()),
        Err(error) => error,
    };
    assert!(error.contains("size_limit"), "{error}");
    Ok(())
}

#[test]
fn foreign_crate_source_root_is_refused() -> Result<(), String> {
    let fixture = tree("foreign-crate")?;
    let src = fixture.root.join("crates/other-crate/src");
    fs::create_dir_all(&src).map_err(|error| error.to_string())?;
    fs::write(src.join("lib.rs"), "pub fn foreign() {}").map_err(|error| error.to_string())?;
    assert!(source_units(&src).is_err());
    Ok(())
}

#[test]
fn standalone_source_files_is_recursive_and_sorted() -> Result<(), String> {
    let fixture = tree("standalone")?;
    let root = fixture.root.join("source");
    fs::create_dir_all(root.join("z/a")).map_err(|error| error.to_string())?;
    fs::write(root.join("z/a/last.rs"), "fn last() {}").map_err(|error| error.to_string())?;
    fs::write(root.join("first.rs"), "fn first() {}").map_err(|error| error.to_string())?;
    fs::write(root.join("readme.md"), "ignored").map_err(|error| error.to_string())?;
    let files = source_files(&root)?;
    let names: Vec<_> = files
        .iter()
        .map(|path| path.strip_prefix(&root).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(names, [Path::new("first.rs"), Path::new("z/a/last.rs")]);
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn symlink_root_ancestor_directory_and_file_are_rejected() -> Result<(), String> {
    let root_link = tree("root-link")?;
    let real = root_link.root.join("real");
    let src = repo_at(&real, "pub fn root() {}")?;
    let linked_src = root_link.root.join("crates/velnor-actions-mise/src");
    fs::create_dir_all(linked_src.parent().ok_or("missing source parent")?)
        .map_err(|error| error.to_string())?;
    symlink(&src, &linked_src).map_err(|error| error.to_string())?;
    assert!(source_units(&linked_src).is_err());

    let ancestor = tree("ancestor-link")?;
    let physical = ancestor.root.join("physical");
    repo_at(&physical, "pub fn root() {}")?;
    let alias = ancestor.root.join("alias");
    symlink(&physical, &alias).map_err(|error| error.to_string())?;
    let alias_src = alias.join("crates/velnor-actions-mise/src");
    assert!(source_units(&alias_src).is_err());

    let nested = tree("nested-links")?;
    let nested_src = repo(&nested, "pub fn root() {}")?;
    let linked_dir = nested_src.join("linked-dir");
    symlink(nested_src.join("nested"), &linked_dir).map_err(|error| error.to_string())?;
    assert!(source_files(&nested_src).is_err());
    fs::remove_file(&linked_dir).map_err(|error| error.to_string())?;
    let linked_file = nested_src.join("linked.rs");
    symlink(nested_src.join("lib.rs"), &linked_file).map_err(|error| error.to_string())?;
    assert!(source_files(&nested_src).is_err());
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn lexical_escape_missing_and_hardlink_aliases_are_rejected() -> Result<(), String> {
    let fixture = tree("invalid-paths")?;
    let src = repo(&fixture, "pub fn root() {}")?;
    let escaped = src.join("..").join("src");
    assert!(source_units(&escaped).is_err());
    let missing = fixture.root.join("crates/velnor-actions-mise/missing");
    assert!(source_files(&missing).is_err());
    let alias = src.join("alias.rs");
    hard_link(src.join("lib.rs"), &alias).map_err(|error| error.to_string())?;
    assert!(source_units(&src).is_err());
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn fifo_like_nonordinary_entry_is_rejected_without_opening_it() -> Result<(), String> {
    let fixture = tree("nonordinary")?;
    let src = repo(&fixture, "pub fn root() {}")?;
    let socket = src.join("fifo");
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    assert!(source_files(&src).is_err());
    drop(listener);
    Ok(())
}
