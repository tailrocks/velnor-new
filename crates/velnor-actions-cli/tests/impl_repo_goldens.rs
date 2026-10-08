//! Golden-tree pins: committed `OpenToFu` goldens omit retired outputs.
//!
//! No golden preview contains `.github/CLAUDE.md`: the generator retired
//! that path (plugin installers reject symlink entries, and a second
//! instruction copy next to `AGENTS.md` carries no benefit). This pin
//! fails closed on any recurrence. Reads repository files only; asserts
//! through content.

use std::error::Error;

use crate::impl_repo_policy::repo_root;

#[test]
fn golden_previews_omit_retired_claude_path() -> Result<(), Box<dyn Error>> {
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
        assert!(
            std::fs::symlink_metadata(&claude).is_err(),
            "golden {name} still carries retired preview/.github/CLAUDE.md",
        );
        let agents = entry.path().join("preview/.github/AGENTS.md");
        assert!(
            std::fs::symlink_metadata(&agents)
                .is_ok_and(|meta| meta.is_file() && !meta.is_symlink()),
            "golden {name} lacks preview/.github/AGENTS.md regular file",
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
