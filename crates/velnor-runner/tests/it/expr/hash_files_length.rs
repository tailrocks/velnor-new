// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, outcome};
use anyhow::Result;
use velnor_runner::context_value::ContextValue;
use velnor_runner::expr::{ExprEnv, RootContext};

const LIMIT: usize = 65_536;
const IMPLICIT_SUFFIX: usize = "/**".len();
const SPECIAL_WORKSPACE: &str = "w[a]*?\\";
const SPECIAL_WORKSPACE_ESCAPES: usize = 7;
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
        let json = serde_json::json!({ "p": pattern }).to_string();
        self.env
            .set_context(RootContext::Env, &ContextValue::from_json_str(&json)?);
        Ok(outcome("hashFiles(env.p)", &self.env))
    }

    fn fits_explicitly(&mut self, units: usize) -> Result<String> {
        let wildcard = "a".repeat(units - "*/**".len());
        self.hash(&format!("*{wildcard}/**"))
    }

    fn fits_implicitly(&mut self, units: usize) -> Result<String> {
        self.hash(&format!("*{}", "a".repeat(units - "*".len())))
    }
}

#[test]
fn expr_hash_files_limits_the_whole_rooted_pattern_not_one_segment() -> Result<()> {
    let mut room = Room::new("ws", 0)?;
    let units = room.units;

    assert_eq!(room.fits_explicitly(units)?, FITS);
    let over = room.fits_explicitly(units + 1)?;
    assert!(over.ends_with(TOO_LONG), "{over}");

    let short_segments = vec!["b".repeat(1_000); 70].join("/");
    let spread = room.hash(&format!("{short_segments}/**"))?;
    assert!(spread.ends_with(TOO_LONG), "{spread}");
    Ok(())
}

#[test]
fn expr_hash_files_counts_the_escaped_working_directory_in_the_limit() -> Result<()> {
    let mut room = Room::new(SPECIAL_WORKSPACE, SPECIAL_WORKSPACE_ESCAPES)?;
    let units = room.units;

    assert_eq!(room.fits_explicitly(units)?, FITS);
    let over = room.fits_explicitly(units + 1)?;
    assert!(over.ends_with(TOO_LONG), "{over}");
    Ok(())
}

#[test]
fn expr_hash_files_limits_the_descendant_pattern_the_runner_adds() -> Result<()> {
    let mut room = Room::new("ws", 0)?;
    let units = room.units;

    assert_eq!(room.fits_implicitly(units - IMPLICIT_SUFFIX)?, FITS);
    let over = room.fits_implicitly(units - IMPLICIT_SUFFIX + 1)?;
    assert!(over.ends_with(TOO_LONG), "{over}");
    assert_eq!(room.fits_explicitly(units)?, FITS);
    Ok(())
}
