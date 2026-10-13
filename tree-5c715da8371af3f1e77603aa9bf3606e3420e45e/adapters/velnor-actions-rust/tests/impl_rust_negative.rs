//! Negative structural cases: dependency surface and write absence.
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Crate manifest directory for source-tree assertions.
fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn manifest_depends_only_on_contract_and_core() {
    let text = std::fs::read_to_string(manifest_dir().join("Cargo.toml"));
    let Ok(text) = text else {
        panic!("crate manifest must be readable");
    };
    let mut in_dependencies = false;
    let mut workspace = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_dependencies = trimmed == "[dependencies]";
            continue;
        }
        if !in_dependencies {
            continue;
        }
        let name = trimmed
            .split(['=', ' ', '\t'])
            .next()
            .unwrap_or_default()
            .trim();
        if name.starts_with("velnor-actions-") {
            workspace.insert(name.to_owned());
        }
    }
    assert_eq!(
        workspace,
        BTreeSet::from([
            "velnor-actions-contract".to_owned(),
            "velnor-actions-contract-config".to_owned(),
            "velnor-actions-contract-planning".to_owned(),
            "velnor-actions-rust-core".to_owned()
        ])
    );
}

/// Filesystem/process tokens that must never appear in adapter sources.
const FORBIDDEN_TOKENS: &[&str] = &[
    "fs::write",
    "fs::remove_file",
    "fs::remove_dir",
    "fs::create_dir",
    "File::create",
    "OpenOptions",
    "Command::new",
    "std::process",
    "fs::rename",
    "fs::copy",
    "fs::set_permissions",
];

#[test]
fn rust_src_never_writes_tool_files() {
    let dirs = [
        manifest_dir().join("src"),
        manifest_dir().join("../velnor-actions-rust-core/src"),
    ];
    let mut checked = 0;
    for src in &dirs {
        let entries = std::fs::read_dir(src);
        let Ok(entries) = entries else {
            panic!("src directory must be readable");
        };
        for entry in entries {
            let Ok(entry) = entry else {
                panic!("src entries must be readable");
            };
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                panic!("source file must be readable: {}", path.display());
            };
            checked += 1;
            for token in FORBIDDEN_TOKENS {
                assert!(
                    !text.contains(token),
                    "forbidden token {token} in {}",
                    path.display()
                );
            }
        }
    }
    assert!(checked > 0, "at least one source file must be scanned");
}
