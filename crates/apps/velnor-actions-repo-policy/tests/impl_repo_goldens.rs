//! Golden-tree pins: committed `OpenToFu` goldens keep generated symlinks.
//!
//! The capture script once used plain `cp -r`, which dereferences symlinks
//! and materialized every golden CLAUDE.md as a regular file (alint
//! `claude-is-agents-symlink` failures). This pin fails closed on any
//! recurrence. Reads repository files only; asserts through content.

use std::error::Error;
use std::path::Path;

use crate::impl_repo_policy::repo_root;

#[test]
fn golden_claude_files_are_agents_symlinks() -> Result<(), Box<dyn Error>> {
    let cases = repo_root().join("crates/apps/velnor-actions-cli/fixtures/opentofu-goldens/cases");
    let mut entries: Vec<_> = std::fs::read_dir(&cases)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut seen = 0;
    for entry in entries {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let link = entry.path().join("preview/.github/CLAUDE.md");
        let meta = std::fs::symlink_metadata(&link)
            .map_err(|_| format!("golden {name} lacks preview/.github/CLAUDE.md"))?;
        assert!(
            meta.is_symlink(),
            "golden {name} CLAUDE.md is a regular file: capture dereferenced the symlink",
        );
        assert_eq!(
            std::fs::read_link(&link)?,
            Path::new("AGENTS.md"),
            "golden {name} CLAUDE.md points at the wrong target",
        );
        seen += 1;
    }
    assert!(
        seen > 0,
        "no golden cases under {} (vacuous pass refused)",
        cases.display(),
    );
    Ok(())
}
