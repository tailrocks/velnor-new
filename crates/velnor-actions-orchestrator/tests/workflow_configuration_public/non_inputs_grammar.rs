// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::json;
use velnor_actions_contract::{ContractError, NonInputs, VelnorConfig};
use velnor_actions_orchestrator::prepare;

use super::scoped_workspace::{TestResult, commit};
use super::workflow_configuration_fixture::make_repo;

const KEY: &str = "stacks.rust.non_inputs";

const ACCEPTED: [&str; 9] = [
    "docs/**",
    "docs/sub/**",
    "a-b/c.d/**",
    "docs/*/**",
    "*/gen/**",
    "Docs/**",
    ".cargo/**",
    "docs.v1/**",
    "_x/**",
];

const REJECTED: [&str; 41] = [
    "docs",
    "docs/",
    ".cargo",
    "shared",
    "docs/*",
    "docs/*.md",
    "*.md",
    "*.lock",
    "Cargo.lock",
    "mise.toml",
    "**",
    "**/docs/**",
    "docs/**/*.md",
    "docs/**/**",
    "docs/**/",
    "docs/***",
    "pics/?.png",
    "a?ps/**",
    "[a]pps/**",
    "[]a]pps/**",
    "docs/[x]/**",
    "{docs,notes}/**",
    "docs/{a,b}/**",
    "do*/**",
    "*docs/**",
    "./x/**",
    "x//y/**",
    "x/./y/**",
    "../x/**",
    "docs/../apps/**",
    "/docs/**",
    "docs\\x/**",
    "!docs/**",
    "my docs/**",
    "a+b/**",
    "a@b/**",
    "a!b/**",
    "é/**",
    "docs/é/**",
    ".../**",
    "",
];

pub(super) fn base_config() -> Result<VelnorConfig, Box<dyn Error>> {
    let repo = make_repo(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\n[stacks.rust]\n",
    )?;
    commit(repo.path(), "base")?;
    Ok(prepare(repo.path())?.config)
}

pub(super) fn load(
    base: &VelnorConfig,
    globs: &[&str],
) -> Result<Result<NonInputs, ContractError>, Box<dyn Error>> {
    let mut config = base.clone();
    let rust = config
        .stacks
        .rust
        .as_mut()
        .ok_or("the base config has no Rust stack")?;
    rust.non_inputs = serde_json::from_value(json!(globs))?;
    Ok(config
        .validate(".velnor/config.toml")
        .map(|()| rust_non_inputs(&config)))
}

fn rust_non_inputs(config: &VelnorConfig) -> NonInputs {
    config
        .stacks
        .rust
        .as_ref()
        .map(|rust| rust.non_inputs.clone())
        .unwrap_or_default()
}

fn problem(error: &ContractError) -> Option<(&str, &str)> {
    match error {
        ContractError::Config {
            key_path, problem, ..
        } => Some((key_path.as_str(), problem.as_str())),
        _ => None,
    }
}

#[test]
fn non_input_grammar_accepts_and_rejects_exactly() -> TestResult {
    let base = base_config()?;
    for glob in ACCEPTED {
        let loaded = load(&base, &[glob])?;
        assert!(loaded.is_ok(), "{glob}: expected Ok, got {loaded:?}");
    }
    for glob in REJECTED {
        match load(&base, &[glob])? {
            Err(error) => assert_eq!(
                problem(&error),
                Some((KEY, format!("bad_glob:{glob}").as_str())),
                "{glob}: expected bad_glob"
            ),
            Ok(accepted) => {
                return Err(format!("{glob}: expected bad_glob, got Ok({accepted:?})").into());
            }
        }
    }
    let duplicate = load(&base, &["docs/**", "docs/**"])?;
    assert_eq!(
        duplicate.as_ref().err().and_then(problem),
        Some((KEY, "duplicate_glob:docs/**")),
        "{duplicate:?}"
    );
    Ok(())
}
