// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, hash_outcome};
use anyhow::Result;
use velnor_runner::expr::ExprEnv;

const LIMIT: usize = 65_536;
const EXPLICIT_SUFFIX: &str = "/**";
const SEGMENT_UNITS: usize = 1_000;
const SPECIAL_WORKSPACE: &str = "w[a]*?\\";
const SPECIAL_WORKSPACE_ESCAPES: usize = 7;
const UNCLOSED_BRACKET_WORKSPACE: &str = "w[]x[y/z]";
const FITS: &str = "ok: ";
const TOO_LONG: &str = "pattern is too long";

struct Room {
    _dir: TempDir,
    env: ExprEnv,
    units: usize,
}

impl Room {
    fn new(name: &str, escapes: usize) -> Result<Self> {
        let dir = TempDir::new()?;
        dir.write(&format!("{name}/a.txt"), "a")?;
        let workspace = std::fs::canonicalize(dir.path().join(name))?;
        let prefix = workspace.to_string_lossy().encode_utf16().count() + escapes + "/".len();
        let env = ExprEnv::new(dir.path().join(name));
        Ok(Self {
            _dir: dir,
            env,
            units: LIMIT - prefix,
        })
    }

    fn hash(&mut self, pattern: &str) -> Result<String> {
        hash_outcome(&mut self.env, pattern)
    }

    fn fits_explicitly(&mut self, units: usize) -> Result<String> {
        self.hash(&format!(
            "{}{EXPLICIT_SUFFIX}",
            spread(units - EXPLICIT_SUFFIX.len())
        ))
    }

    fn fits_implicitly(&mut self, units: usize) -> Result<String> {
        self.hash(&spread(units))
    }
}

fn assert_too_long(outcome: &str, units: usize) {
    assert!(
        outcome.ends_with(TOO_LONG),
        "a pattern of {units} units is one over the {LIMIT}-unit limit once the working directory is counted, so the call should fail with {TOO_LONG:?}, but it ended: {outcome:?}"
    );
}

fn spread(units: usize) -> String {
    let mut text = String::from("*");
    let mut left = units - text.len();
    while left > 0 {
        let mut length = (left - 1).min(SEGMENT_UNITS);
        if left - length - 1 == 1 {
            length -= 1;
        }
        text.push('/');
        text.push_str(&"b".repeat(length));
        left -= length + 1;
    }
    text
}

#[test]
fn expr_hash_files_limits_the_whole_rooted_pattern_not_one_segment() -> Result<()> {
    let mut room = Room::new("ws", 0)?;
    let units = room.units;

    assert_eq!(room.fits_explicitly(units)?, FITS);
    assert_too_long(&room.fits_explicitly(units + 1)?, units + 1);
    Ok(())
}

#[test]
fn expr_hash_files_counts_the_escaped_working_directory_in_the_limit() -> Result<()> {
    let mut room = Room::new(SPECIAL_WORKSPACE, SPECIAL_WORKSPACE_ESCAPES)?;
    let units = room.units;

    assert_eq!(room.fits_explicitly(units)?, FITS);
    assert_too_long(&room.fits_explicitly(units + 1)?, units + 1);
    Ok(())
}

#[test]
fn expr_hash_files_leaves_a_working_directory_bracket_unescaped_when_no_class_closes() -> Result<()>
{
    let mut room = Room::new(UNCLOSED_BRACKET_WORKSPACE, 0)?;
    let units = room.units;

    assert_eq!(room.fits_explicitly(units)?, FITS);
    assert_too_long(&room.fits_explicitly(units + 1)?, units + 1);
    Ok(())
}

#[test]
fn expr_hash_files_limits_the_descendant_pattern_the_runner_adds() -> Result<()> {
    let mut room = Room::new("ws", 0)?;
    let units = room.units;

    assert_eq!(room.fits_implicitly(units - EXPLICIT_SUFFIX.len())?, FITS);
    assert_too_long(
        &room.fits_implicitly(units - EXPLICIT_SUFFIX.len() + 1)?,
        units + 1,
    );
    assert_eq!(room.fits_explicitly(units)?, FITS);
    Ok(())
}
