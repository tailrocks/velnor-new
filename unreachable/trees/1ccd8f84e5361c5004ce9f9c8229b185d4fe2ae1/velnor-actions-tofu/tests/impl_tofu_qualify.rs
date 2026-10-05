//! Module boundary: H5 canonicalization plus full qualification.
use std::collections::BTreeSet;
use std::path::PathBuf;

use velnor_actions_tofu::effective::effective_set;
use velnor_actions_tofu::modules::{
    ModuleEdge, ModuleError, ModuleRef, ModuleSource, qualify_module_edges, resolve_refs,
};
use velnor_actions_tofu::units::analyze_files;

use crate::support::{Outcome, TempDir, fixture_dir, read_pairs};

/// One literal module reference from `file`.
fn literal(file: &str, name: &str, source: &str) -> ModuleRef {
    ModuleRef {
        file: file.to_owned(),
        name: name.to_owned(),
        source: ModuleSource::Literal(source.to_owned()),
    }
}

/// Edge target multiset, sorted.
fn targets(edges: &[ModuleEdge]) -> Vec<&str> {
    let mut targets: Vec<&str> = edges.iter().map(|edge| edge.to.as_str()).collect();
    targets.sort_unstable();
    targets
}

/// Fixture refs plus the repo file list and root.
type FixtureRefs = (Vec<ModuleRef>, Vec<String>, PathBuf);

/// Analyzed refs plus the repo file list for fixture `name`.
fn fixture_refs(name: &str) -> Result<FixtureRefs, Box<dyn std::error::Error>> {
    let dir = fixture_dir(name);
    let all = read_pairs(&dir, "")?;
    let files: Vec<String> = all.iter().map(|(path, _)| path.clone()).collect();
    let effective = effective_set(&files);
    let selected: Vec<(String, String)> = all
        .into_iter()
        .filter(|(path, _)| effective.contains(path))
        .collect();
    let unit = analyze_files(&selected)?;
    Ok((unit.modules, files, dir))
}

#[test]
fn diamond_fixture_resolves_to_one_shared_dir() -> Outcome {
    let (refs, _, _) = fixture_refs("tofu-modules")?;
    let resolved = resolve_refs(&refs).expect("diamond resolves");
    // Root calls a/b, a calls nested/shared, b calls shared: five edges.
    assert_eq!(resolved.edges.len(), 5);
    assert!(resolved.findings.is_empty());
    // Both `../shared` references land on ONE directory.
    let shared = targets(&resolved.edges)
        .into_iter()
        .filter(|target| *target == "modules/shared")
        .count();
    assert_eq!(shared, 2, "two edges, one shared dir");
    let mut dirs: BTreeSet<String> = resolved.edges.iter().map(|edge| edge.to.clone()).collect();
    dirs.retain(|dir| dir.ends_with("shared"));
    assert_eq!(dirs.len(), 1);
    Ok(())
}

#[test]
fn remote_fixture_records_kinds() -> Outcome {
    let (refs, _, _) = fixture_refs("tofu-remote")?;
    let resolved = resolve_refs(&refs).expect("remote resolves");
    assert!(resolved.edges.is_empty());
    assert_eq!(resolved.findings.len(), 2);
    let mut kinds: Vec<String> = resolved
        .findings
        .iter()
        .map(|finding| format!("{:?}", finding.class))
        .collect();
    kinds.sort();
    assert_eq!(kinds, vec!["Remote(Git)", "Remote(Registry)"]);
    Ok(())
}

#[test]
fn qualify_accepts_diamond_at_fixture_root() -> Outcome {
    let (refs, files, dir) = fixture_refs("tofu-modules")?;
    let qualified = qualify_module_edges(&dir, &files, &refs).expect("diamond qualifies");
    assert_eq!(qualified.edges.len(), 5);
    assert!(qualified.findings.is_empty());
    Ok(())
}

#[test]
fn qualify_remote_passes_findings_through() -> Outcome {
    let (refs, files, dir) = fixture_refs("tofu-remote")?;
    let qualified = qualify_module_edges(&dir, &files, &refs).expect("remote qualifies");
    assert!(qualified.edges.is_empty());
    assert_eq!(qualified.findings.len(), 2);
    Ok(())
}

#[test]
fn qualify_missing_absent_target_errors() -> Outcome {
    let root = TempDir::create("tofu-qualify-absent")?;
    root.write("main.tf", "module \"a\" {\n  source = \"./mods/a\"\n}\n")?;
    let refs = vec![literal("main.tf", "a", "./mods/a")];
    let err = qualify_module_edges(root.path(), &["main.tf".to_owned()], &refs)
        .expect_err("absent errors");
    assert!(matches!(err, ModuleError::MissingTarget { .. }), "{err}");
    assert!(err.to_string().contains("mods/a"), "{err}");
    Ok(())
}

#[test]
fn qualify_config_free_target_errors() -> Outcome {
    let root = TempDir::create("tofu-qualify-empty")?;
    root.write("main.tf", "module \"a\" {\n  source = \"./mods/a\"\n}\n")?;
    root.write("mods/a/README.md", "no config\n")?;
    let refs = vec![literal("main.tf", "a", "./mods/a")];
    let files = vec!["main.tf".to_owned(), "mods/a/README.md".to_owned()];
    let err = qualify_module_edges(root.path(), &files, &refs).expect_err("config-free errors");
    assert!(matches!(err, ModuleError::MissingTarget { .. }), "{err}");
    Ok(())
}

#[test]
fn qualify_lexical_escape_errors() -> Outcome {
    let root = TempDir::create("tofu-qualify-escape")?;
    root.write("a/main.tf", "module \"o\" {\n  source = \"../../o\"\n}\n")?;
    let refs = vec![literal("a/main.tf", "o", "../../o")];
    let err = qualify_module_edges(root.path(), &["a/main.tf".to_owned()], &refs)
        .expect_err("escape errors");
    assert!(matches!(err, ModuleError::Escape { .. }), "{err}");
    Ok(())
}

#[test]
fn qualify_cycle_errors() -> Outcome {
    let root = TempDir::create("tofu-qualify-cycle")?;
    root.write("a/main.tf", "module \"b\" {\n  source = \"../b\"\n}\n")?;
    root.write("b/main.tf", "module \"a\" {\n  source = \"../a\"\n}\n")?;
    let refs = vec![
        literal("a/main.tf", "b", "../b"),
        literal("b/main.tf", "a", "../a"),
    ];
    let files = vec!["a/main.tf".to_owned(), "b/main.tf".to_owned()];
    let err = qualify_module_edges(root.path(), &files, &refs).expect_err("cycle errors");
    assert!(matches!(err, ModuleError::Cycle { .. }), "{err}");
    Ok(())
}

#[test]
#[cfg(unix)]
fn qualify_escaping_symlink_rejected() -> Outcome {
    let root = TempDir::create("tofu-qualify-link-escape")?;
    let outside = TempDir::create("tofu-qualify-link-outside")?;
    outside.write("main.tf", "variable \"x\" {}\n")?;
    root.write("main.tf", "module \"a\" {\n  source = \"./link\"\n}\n")?;
    std::os::unix::fs::symlink(outside.path(), root.path().join("link"))?;
    let refs = vec![literal("main.tf", "a", "./link")];
    let err = qualify_module_edges(root.path(), &["main.tf".to_owned()], &refs)
        .expect_err("escape errors");
    assert!(matches!(err, ModuleError::Escape { .. }), "{err}");
    Ok(())
}

#[test]
#[cfg(unix)]
fn qualify_in_repo_symlink_canonicalizes() -> Outcome {
    let root = TempDir::create("tofu-qualify-link")?;
    root.write("real/main.tf", "variable \"x\" {}\n")?;
    root.write("main.tf", "module \"a\" {\n  source = \"./link\"\n}\n")?;
    std::os::unix::fs::symlink(root.path().join("real"), root.path().join("link"))?;
    let refs = vec![literal("main.tf", "a", "./link")];
    let files = vec!["main.tf".to_owned(), "real/main.tf".to_owned()];
    let qualified = qualify_module_edges(root.path(), &files, &refs).expect("in-repo link ok");
    assert_eq!(qualified.edges.len(), 1);
    assert_eq!(qualified.edges[0].to, "real");
    assert_eq!(qualified.edges[0].source, "./link", "spelling kept");
    Ok(())
}

#[test]
fn qualify_unreadable_root_errors() {
    let refs = vec![literal("main.tf", "a", "./mods/a")];
    let err = qualify_module_edges(
        std::path::Path::new("/nonexistent-velnor-root"),
        &["main.tf".to_owned()],
        &refs,
    )
    .expect_err("bad root errors");
    assert!(matches!(err, ModuleError::Unreadable { .. }), "{err}");
    assert_eq!(err.to_string(), "unreadable_module_root");
}
