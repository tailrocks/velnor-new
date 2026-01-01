//! Repository source and documentation size limits.

use std::error::Error;

use crate::impl_repo_policy::{MEMBERS, read, tree_files};

/// Physical lines: newline count, matching `wc -l`.
pub(crate) fn physical_lines(body: &str) -> usize {
    body.bytes().filter(|byte| *byte == b'\n').count()
}

#[test]
fn size_limits_hold() -> Result<(), Box<dyn Error>> {
    let mut over = Vec::new();
    for (dir, _) in MEMBERS {
        for area in ["src", "tests"] {
            for path in tree_files(&format!("{dir}/{area}"), "rs")? {
                let lines = physical_lines(&std::fs::read_to_string(&path)?);
                if lines > 400 {
                    over.push(format!("{} ({lines})", path.display()));
                }
                let name = path
                    .file_name()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("");
                if (name == "lib.rs" || name == "main.rs") && lines > 150 {
                    over.push(format!("{} lib/main ({lines})", path.display()));
                }
            }
        }
    }
    assert!(over.is_empty(), "over 400 lines: {}", over.join(", "));
    assert!(read("clippy.toml")?.contains("too-many-lines-threshold = 80"));
    assert!(read("Cargo.toml")?.contains("too_many_lines = \"deny\""));
    let mut docs = tree_files("docs", "md")?;
    docs.extend(tree_files(".velnor", "toml")?);
    docs.extend(tree_files(".velnor", "json")?);
    for path in docs {
        let lines = physical_lines(&std::fs::read_to_string(&path)?);
        assert!(lines <= 400, "{} has {lines} lines", path.display());
    }
    Ok(())
}
