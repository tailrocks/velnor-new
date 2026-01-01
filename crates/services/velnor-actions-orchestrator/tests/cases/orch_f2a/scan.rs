//! F2 scan helpers: family sources, file lookup, and code lines.

use std::path::{Path, PathBuf};

/// Family `src/` directories: the hub plus every extracted sibling crate.
///
/// Structural scans span the family so moved modules stay covered.
/// Membership is the `velnor-actions-orchestrator` prefix (the hub
/// itself plus every `orchestrator-` sibling), so later extractions
/// join the scan without touching this file.
fn family_src_dirs() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(services) = manifest.parent() else {
        return Ok(vec![manifest.join("src")]);
    };
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(services)? {
        let member = entry?.path();
        let name = member
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name == "velnor-actions-orchestrator" || name.starts_with("velnor-actions-orchestrator-")
        {
            dirs.push(member.join("src"));
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// The one family `src/<name>` file; extractions move files across siblings.
pub(crate) fn family_file(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let hits: Vec<PathBuf> = family_src_dirs()?
        .iter()
        .map(|dir| dir.join(name))
        .filter(|path| path.is_file())
        .collect();
    if hits.len() == 1 {
        hits.into_iter()
            .next()
            .ok_or_else(|| format!("no match for {name}").into())
    } else {
        Err(format!("{} matches for {name}", hits.len()).into())
    }
}

/// Sorted `.rs` files directly under every family `src/`.
pub(crate) fn src_files() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for dir in family_src_dirs()? {
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Strip a trailing `//` comment, ignoring `//` inside string literals.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quoted = false;
    let mut escape = false;
    let mut index = 0;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        if escape {
            escape = false;
        } else if byte == b'\\' && quoted {
            escape = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'/' && bytes[index + 1] == b'/' && !quoted {
            return line[..index].trim_end();
        }
        index += 1;
    }
    line
}

/// Code lines of one file: `(number, code)` with comments stripped.
pub(crate) fn code_of(path: &Path) -> Result<Vec<(usize, String)>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .enumerate()
        .map(|(number, line)| (number + 1, strip_line_comment(line).to_owned()))
        .filter(|(_, line)| !line.trim().is_empty())
        .collect())
}
