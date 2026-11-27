// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use velnor_runner::expr::segment_matches;

const CORPUS: &str = include_str!("minimatch_corpus.json");
const PATTERNS: usize = 2600;
const CASES: usize = 52082;
const REFUSED_PATTERNS: usize = 4;
const REPORTED_MISMATCHES: usize = 10;

#[test]
fn expr_hash_files_segments_agree_with_minimatch_3_1_2_on_the_recorded_corpus() -> Result<()> {
    let entries: Vec<Vec<String>> = serde_json::from_str(CORPUS).context("parse corpus")?;
    let (mut cases, mut refused) = (0, 0);
    let mut mismatches = Vec::new();
    for entry in &entries {
        let [pattern, bits, names @ ..] = entry.as_slice() else {
            bail!("corpus entry {entry:?} is too short");
        };
        if bits == "refused" {
            refused += 1;
            let answer = segment_matches(pattern, "a");
            if !answer
                .as_ref()
                .is_err_and(|message| message.ends_with("which is not supported"))
            {
                mismatches.push(format!("{pattern:?} should be refused, got {answer:?}"));
            }
            continue;
        }
        if bits.len() != names.len() {
            bail!(
                "corpus entry {pattern:?} has {} bits for {} names",
                bits.len(),
                names.len()
            );
        }
        for (name, bit) in names.iter().zip(bits.chars()) {
            cases += 1;
            let expected = bit == '1';
            let answer = segment_matches(pattern, name);
            if answer != Ok(expected) {
                mismatches.push(format!(
                    "{pattern:?} against {name:?}: minimatch says {expected}, got {answer:?}"
                ));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatches, first {REPORTED_MISMATCHES}:\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .take(REPORTED_MISMATCHES)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        (entries.len(), cases, refused),
        (PATTERNS, CASES, REFUSED_PATTERNS)
    );
    Ok(())
}
