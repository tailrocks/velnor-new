//! Golden-tree pins: committed `OpenToFu` goldens keep generated pointers.
//!
//! Generated `CLAUDE.md` files are regular files with the single
//! `@AGENTS.md` import line, never symlinks: plugin installers reject
//! packages that contain symlink entries. This pin fails closed on any
//! symlink recurrence or content drift. Reads repository files only;
//! asserts through content.

use std::error::Error;

use crate::impl_repo_policy::repo_root;

#[test]
fn golden_claude_files_are_agents_pointers() -> Result<(), Box<dyn Error>> {
    let cases = repo_root().join("docs/proposed/opentofu-goldens/cases");
    let mut entries: Vec<_> = std::fs::read_dir(&cases)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut seen = 0;
    for entry in entries {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let claude = entry.path().join("preview/.github/CLAUDE.md");
        let meta = std::fs::symlink_metadata(&claude)
            .map_err(|_| format!("golden {name} lacks preview/.github/CLAUDE.md"))?;
        assert!(
            !meta.is_symlink(),
            "golden {name} CLAUDE.md is a symlink: generator must emit a regular file",
        );
        assert!(
            meta.is_file(),
            "golden {name} CLAUDE.md is not a regular file",
        );
        assert_eq!(
            std::fs::read(&claude)?,
            b"@AGENTS.md\n",
            "golden {name} CLAUDE.md is not the @AGENTS.md pointer body",
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
