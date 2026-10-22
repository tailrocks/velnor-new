//! Full-tree byte identity for the parity corpus: every file `generate`
//! writes (composite actions, every workflow, lint config, agent pointers),
//! not only `ci.yml`, is pinned by SHA-256 in `expected/generate-tree.sha256`.
//!
//! None of these repositories opts into `[upgrades]`, so a renderer change
//! that alters any of their bytes (a new composite step, a changed script)
//! fails here until the golden is deliberately refreshed with
//! `VELNOR_UPDATE_GOLDENS=1`, and the refresh diff names every changed file.

use std::error::Error;
use std::fmt::Write as _;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::impl_cli_parity_golden::check_golden;

/// `<sha256>  <path>` per generated file, `symlink <target>  <path>` per
/// link, sorted by path.
fn manifest(preview: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut rows = Vec::new();
    walk(preview, preview, &mut rows)?;
    rows.sort_by(|left: &(String, String), right| left.0.cmp(&right.0));
    let mut out = String::new();
    for (path, digest) in rows {
        writeln!(out, "{digest}  {path}")?;
    }
    Ok(out.into_bytes())
}

fn walk(root: &Path, dir: &Path, rows: &mut Vec<(String, String)>) -> Result<(), Box<dyn Error>> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("non-utf8 generated path")?
            .to_owned();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            let target = std::fs::read_link(&path)?;
            rows.push((relative, format!("symlink {}", target.display())));
        } else if kind.is_dir() {
            walk(root, &path, rows)?;
        } else {
            let digest = Sha256::digest(std::fs::read(&path)?);
            let mut hex = String::with_capacity(64);
            for byte in digest {
                write!(hex, "{byte:02x}")?;
            }
            rows.push((relative, hex));
        }
    }
    Ok(())
}

/// Compare the whole generated tree of `case` against its golden manifest.
pub(crate) fn check(case: &str, preview: &Path) -> Result<(), Box<dyn Error>> {
    let manifest = manifest(preview)?;
    if manifest.is_empty() {
        return Err(format!("{case}: generate wrote no files (vacuous pass refused)").into());
    }
    check_golden(case, "generate-tree.sha256", &manifest)
}
