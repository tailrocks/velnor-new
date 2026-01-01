//! Rendered trees materialize exclusively with symlink-safe parents.

use velnor_actions_orchestrator_staged_validation::write::write_tree;
use velnor_actions_workflow_tree::rendered::{RenderedFile, RenderedSymlink, RenderedTree};

fn tree() -> RenderedTree {
    RenderedTree {
        files: vec![
            RenderedFile {
                path: ".github/workflows/ci.yml".into(),
                bytes: "jobs: {}\n".into(),
            },
            RenderedFile {
                path: ".github/actionlint.yaml".into(),
                bytes: "config: true\n".into(),
            },
        ],
        symlinks: vec![RenderedSymlink {
            path: ".github/CLAUDE.md".into(),
            target: "../AGENTS.md".into(),
        }],
    }
}

#[test]
fn round_trip_writes_files_and_symlinks() {
    let root = tempfile::TempDir::new().expect("root");
    let github = root.path().join(".github");
    write_tree(&github, &tree()).expect("write");
    assert_eq!(
        std::fs::read_to_string(github.join("workflows/ci.yml")).expect("read"),
        "jobs: {}\n"
    );
    assert_eq!(
        std::fs::read_to_string(github.join("actionlint.yaml")).expect("read"),
        "config: true\n"
    );
    let link = github.join("CLAUDE.md");
    assert_eq!(
        std::fs::read_link(&link).expect("link"),
        std::path::Path::new("../AGENTS.md")
    );
}

#[test]
fn existing_file_refuses_overwrite() {
    let root = tempfile::TempDir::new().expect("root");
    let github = root.path().join(".github");
    std::fs::create_dir_all(github.join("workflows")).expect("dirs");
    std::fs::write(github.join("workflows/ci.yml"), "old\n").expect("old");
    assert!(write_tree(&github, &tree()).is_err());
    assert_eq!(
        std::fs::read_to_string(github.join("workflows/ci.yml")).expect("read"),
        "old\n"
    );
}

#[test]
fn existing_symlink_at_file_destination_refuses() {
    let root = tempfile::TempDir::new().expect("root");
    let github = root.path().join(".github");
    std::fs::create_dir_all(github.join("workflows")).expect("dirs");
    #[cfg(unix)]
    std::os::unix::fs::symlink("elsewhere", github.join("workflows/ci.yml")).expect("link");
    #[cfg(not(unix))]
    std::fs::write(github.join("workflows/ci.yml"), "old\n").expect("old");
    assert!(write_tree(&github, &tree()).is_err());
}

#[test]
fn second_write_refuses_existing_tree() {
    let root = tempfile::TempDir::new().expect("root");
    let github = root.path().join(".github");
    write_tree(&github, &tree()).expect("first");
    assert!(write_tree(&github, &tree()).is_err());
}
