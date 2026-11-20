// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{RepoPath, RustCiConfig, RustRoot, SetupPhase, TestSetup};

use crate::config::CONFIG_PATH;
use crate::error::{DetectError, Reason};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTestSetup {
    root: String,
    name: String,
    run: String,
    phase: Option<String>,
}

pub(crate) fn parse(raw: Vec<RawTestSetup>) -> Result<Vec<TestSetup>, Reason> {
    raw.into_iter().map(setup).collect()
}

fn setup(raw: RawTestSetup) -> Result<TestSetup, Reason> {
    let text = |key: &'static str, value: &str| {
        let clean = !value.trim().is_empty()
            && !value.contains("${{")
            && !value.chars().any(char::is_control);
        if clean {
            Ok(())
        } else {
            Err(Reason::InvalidValue {
                key,
                value: value.to_owned(),
            })
        }
    };
    text("test_setup.name", &raw.name)?;
    text("test_setup.run", &raw.run)?;
    let root = match raw.root.as_str() {
        "." => None,
        directory => Some(RepoPath::new(directory.trim_end_matches('/')).map_err(|_| {
            Reason::InvalidValue {
                key: "test_setup.root",
                value: raw.root.clone(),
            }
        })?),
    };
    let phase = raw
        .phase
        .as_deref()
        .map(|phase| match phase {
            "test" => Ok(SetupPhase::Test),
            "build" => Ok(SetupPhase::Build),
            other => Err(Reason::InvalidValue {
                key: "test_setup.phase",
                value: other.to_owned(),
            }),
        })
        .transpose()?;
    Ok(TestSetup {
        root,
        name: raw.name,
        run: raw.run,
        phase,
    })
}

pub(crate) fn check(config: &RustCiConfig, rust_roots: &[RustRoot]) -> Result<(), DetectError> {
    match config.test_setup.iter().find(|setup| {
        !rust_roots
            .iter()
            .any(|root| root.manifest.parent() == setup.root)
    }) {
        Some(setup) => Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::UnknownTestSetupRoot(
                setup
                    .root
                    .as_ref()
                    .map_or_else(|| ".".to_owned(), ToString::to_string),
            ),
        )),
        None => Ok(()),
    }
}
