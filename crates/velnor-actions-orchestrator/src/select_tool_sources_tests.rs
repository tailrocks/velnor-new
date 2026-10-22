use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::{PROJECT_TOOL_CONFIG_CHANGED, unchanged_declaration_sources};
use crate::git_fixture;

fn git(root: &Path, args: &[&str]) {
    let status = git_fixture::command(root)
        .expect("git command")
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?}");
}

fn commit(root: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@e.c",
            "commit",
            "-q",
            "-m",
            message,
        ],
    );
    let output = git_fixture::command(root)
        .expect("git command")
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse");
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .to_owned()
}

fn changed(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

fn verdict(
    before: &[(&str, &str)],
    after: &[(&str, &str)],
    paths: &[&str],
) -> (Option<BTreeSet<String>>, Vec<String>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    let base = commit(root, before, "base");
    let head = commit(root, after, "head");
    let mut warnings = Vec::new();
    let out =
        unchanged_declaration_sources(root, &base, Some(&head), &changed(paths), &mut warnings);
    (out, warnings)
}

#[test]
fn equal_declarations_are_unchanged_and_different_ones_broaden() {
    let tools = |rust: &str, node: &str, task: &str| {
        format!("[tools]\nrust = \"{rust}\"\nnode = \"{node}\"\n[tasks.a]\nrun = \"{task}\"\n")
    };
    let (out, warnings) = verdict(
        &[("mise.toml", &tools("1.99.1", "20", "echo a"))],
        &[("mise.toml", &tools("1.99.1", "22", "echo b"))],
        &["mise.toml"],
    );
    assert_eq!(out, Some(changed(&["mise.toml"])));
    assert!(warnings.is_empty(), "{warnings:?}");
    let (out, warnings) = verdict(
        &[("mise.toml", &tools("1.99.1", "20", "echo a"))],
        &[("mise.toml", &tools("1.99.2", "20", "echo a"))],
        &["mise.toml"],
    );
    assert_eq!(out, None);
    assert_eq!(warnings, [PROJECT_TOOL_CONFIG_CHANGED]);
}

#[test]
fn every_backend_spelling_and_table_form_counts_as_a_declaration() {
    let key = "\"aqua:nextest-rs/nextest/cargo-nextest\"";
    for (before, after) in [
        (
            format!("[tools]\n{key} = \"0.9.1\"\n"),
            format!("[tools]\n{key} = \"0.9.2\"\n"),
        ),
        (
            format!("[tools]\n{key} = {{ version = \"0.9.1\" }}\n"),
            format!("[tools]\n{key} = {{ version = \"0.9.2\", os = [\"linux\"] }}\n"),
        ),
        (
            "[tools]\ncargo-nextest = \"0.9.1\"\n".to_owned(),
            format!("[tools]\n{key} = \"0.9.1\"\n"),
        ),
        (
            "[tools]\nshellcheck = \"0.11.0\"\n".to_owned(),
            "[tools]\nshellcheck = [\"0.11.0\"]\n".to_owned(),
        ),
    ] {
        let (out, _) = verdict(
            &[("mise.toml", &before)],
            &[("mise.toml", &after)],
            &["mise.toml"],
        );
        assert_eq!(out, None, "{before} -> {after}");
    }
}

#[test]
fn doubt_broadens() {
    // Unparseable head, lock files, and a path git cannot find on either side.
    let (out, _) = verdict(
        &[("mise.toml", "[tools]\nrust = \"1.99.1\"\n")],
        &[("mise.toml", "[tools\n")],
        &["mise.toml"],
    );
    assert_eq!(out, None);
    let (out, _) = verdict(
        &[("mise.lock", "a\n")],
        &[("mise.lock", "b\n")],
        &["mise.lock"],
    );
    assert_eq!(out, None);
    let (out, _) = verdict(&[("x", "a\n")], &[("x", "b\n")], &["mise.toml"]);
    assert_eq!(out, None);
}

#[test]
fn other_files_are_not_declaration_sources() {
    let (out, warnings) = verdict(&[("x", "a\n")], &[("x", "b\n")], &["x", "src/lib.rs"]);
    assert_eq!(out, Some(BTreeSet::new()));
    assert!(warnings.is_empty(), "{warnings:?}");
}
