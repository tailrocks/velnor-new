// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct Plan {
    pub(crate) rerun: Vec<PathBuf>,
    pub(crate) unwatched: Vec<String>,
}

pub(crate) fn plan(
    watched: Vec<PathBuf>,
    absent: Vec<(String, PathBuf)>,
    out_dir: Option<&Path>,
) -> Plan {
    let unwatchable = |source: &Path| out_dir.is_some_and(|out| walk_may_reach(source, out));
    let unwatched = absent
        .into_iter()
        .filter(|(_, nearest)| unwatchable(nearest))
        .map(|(relative, _)| relative)
        .collect();
    let rerun = watched
        .into_iter()
        .filter(|source| !unwatchable(source))
        .collect();
    Plan { rerun, unwatched }
}

fn walk_may_reach(source: &Path, out_dir: &Path) -> bool {
    if out_dir.starts_with(source) {
        return true;
    }
    let (Ok(real_source), Ok(real_out)) = (source.canonicalize(), out_dir.canonicalize()) else {
        return false;
    };
    real_out.starts_with(&real_source) || links_reach(&real_source, &real_out)
}

fn links_reach(directory: &Path, real_out: &Path) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    entries.into_iter().flatten().any(|entry| {
        let path = entry.path();
        fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink())
            && path
                .canonicalize()
                .is_ok_and(|real| real_out.starts_with(real))
    })
}

#[cfg(test)]
mod tests {
    use super::plan;
    use std::error::Error;
    use std::fs;
    use std::os::unix::fs::symlink;

    #[test]
    fn unresolvable_paths_are_watched_never_dropped() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let base = temp.path().canonicalize()?;
        let root = base.join("root");
        fs::create_dir(&root)?;
        fs::create_dir(base.join("out"))?;
        symlink(base.join("missing"), root.join("dangling"))?;
        let absent = vec![("rust-toolchain".to_owned(), root.clone())];
        for out in [base.join("out"), base.join("no-such-out")] {
            let planned = plan(vec![root.clone()], absent.clone(), Some(&out));
            assert_eq!(planned.rerun, vec![root.clone()]);
            assert!(planned.unwatched.is_empty());
        }
        Ok(())
    }

    #[test]
    fn a_target_the_walk_reaches_is_unwatched() -> Result<(), Box<dyn Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        fs::create_dir_all(root.join("target/debug/out"))?;
        let absent = vec![("rust-toolchain".to_owned(), root.clone())];
        let planned = plan(
            vec![root.clone()],
            absent,
            Some(&root.join("target/debug/out")),
        );
        assert!(planned.rerun.is_empty());
        assert_eq!(planned.unwatched, vec!["rust-toolchain".to_owned()]);
        Ok(())
    }
}
