//! Golden-tree pins: committed `OpenToFu` goldens keep generated mirrors.
//!
//! Generated `CLAUDE.md` files are regular files with bytes identical to
//! their sibling `AGENTS.md`, never symlinks: plugin installers reject
//! packages that contain symlink entries. This pin fails closed on any
//! symlink recurrence or content drift. Reads repository files only;
//! asserts through content.

use std::error::Error;
use std::path::Path;

use crate::impl_repo_policy::repo_root;

#[test]
fn golden_claude_files_mirror_agents_bytes() -> Result<(), Box<dyn Error>> {
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
        let agents = entry.path().join("preview/.github/AGENTS.md");
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
            std::fs::read(&agents).map_err(|_| format!(
                "golden {name} lacks preview/.github/AGENTS.md ({})",
                Path::new("preview/.github/AGENTS.md").display()
            ))?,
            "golden {name} CLAUDE.md bytes differ from AGENTS.md",
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
