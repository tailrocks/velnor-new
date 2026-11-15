// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

type TestResult = Result<(), Box<dyn Error>>;

fn workflow_names(dir: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(dir)? {
        let text = fs::read_to_string(entry?.path())?;
        if let Some(name) = text.lines().find_map(|line| line.strip_prefix("name: ")) {
            names.insert(name.trim().to_owned());
        }
    }
    Ok(names)
}

fn triggers(release: &str) -> Vec<String> {
    release
        .lines()
        .skip_while(|line| *line != "    workflows:")
        .skip(1)
        .take_while(|line| line.starts_with("      - "))
        .map(|line| line.trim_start_matches("      - ").to_owned())
        .collect()
}

#[test]
fn every_release_golden_triggers_after_a_workflow_this_repository_has() -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let known = workflow_names(&root.join(".github/workflows"))?;
    let mut checked = 0;
    for sample in fs::read_dir(root.join("generator/samples"))? {
        let release = sample?
            .path()
            .join("expected/.github/workflows/release.yml");
        let Ok(text) = fs::read_to_string(&release) else {
            continue;
        };
        let names = triggers(&text);
        assert!(!names.is_empty(), "{}", release.display());
        for name in names {
            assert!(known.contains(&name), "{}: {name}", release.display());
        }
        checked += 1;
    }
    assert!(checked >= 2);
    Ok(())
}
