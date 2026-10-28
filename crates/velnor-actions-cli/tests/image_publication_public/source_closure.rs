// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, error::Error, fs};

use serde_json::Value;

use super::support::{Fixture, FixtureProfile};

const EXPECTED_RUNNER_INPUTS: [&str; 21] = [
    ".velnor/runner.lock",
    "images/dind/Dockerfile",
    "images/dind/LICENSE-moby",
    "images/dind/cgroup-v2-delegation.sh",
    "images/dind/entrypoint.sh",
    "images/runner/ubuntu-26.04/Dockerfile",
    "images/runner/ubuntu-26.04/clear-dockerenv.sh",
    "images/runner/ubuntu-26.04/entrypoint.sh",
    "images/runner/ubuntu-26.04/tar-absolute.sh",
    "images/runner/ubuntu-26.04/tar-dir-meta.pl",
    "images/runner/ubuntu-26.04/tar-extract-plan.sh",
    "images/runner/ubuntu-26.04/tar-extract.sh",
    "images/runner/ubuntu-26.04/tar-member-rewrite.pl",
    "images/runner/ubuntu-26.04/tar-member-stream.pl",
    "images/runner/ubuntu-26.04/tar-member.pl",
    "images/runner/ubuntu-26.04/tar-pax.pl",
    "images/runner/ubuntu-26.04/tar-shim.sh",
    "images/runner/ubuntu-26.04/wait-docker-api.sh",
    "images/runner/ubuntu-26.04/wait-docker-sock.sh",
    "images/resource-probe/Dockerfile",
    "images/resource-probe/build-image.sh",
];

#[test]
fn public_runner_family_accepts_the_independent_complete_input_inventory()
-> Result<(), Box<dyn Error>> {
    let expected = EXPECTED_RUNNER_INPUTS
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    for profile in runner_profiles() {
        let fixture = Fixture::new_for(profile)?;
        let record: Value = serde_json::from_slice(&fs::read(&fixture.record)?)?;
        let actual = record["source"]["inputs"]
            .as_object()
            .ok_or("publication record has no source input map")?
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected, "{} source inventory", profile.name());
        let output = fixture.invoke()?;
        assert!(
            output.status.success(),
            "{} publication failed: {}",
            profile.name(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(fixture.last_fixture_error()?.is_none());
        assert_eq!(fixture.registry_digest()?, Some(fixture.expected_digest));
    }
    Ok(())
}

#[test]
fn public_runner_family_rejects_drift_in_previously_omitted_copy_inputs()
-> Result<(), Box<dyn Error>> {
    for (profile, path) in [
        (
            FixtureProfile::Runner,
            "images/runner/ubuntu-26.04/entrypoint.sh",
        ),
        (FixtureProfile::Dind, "images/dind/entrypoint.sh"),
    ] {
        let fixture = Fixture::new_for(profile)?;
        let registry_before = fixture.preseed_registry_state()?;
        fixture.set_source_response(path, b"changed copied source\n")?;
        let output = fixture.invoke()?;
        assert!(
            !output.status.success(),
            "{} accepted changed input {path}",
            profile.name()
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("image_publication_source_digest"),
            "unexpected source rejection: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(fixture.last_fixture_error()?.is_none());
        assert!(!fixture.receipt.exists());
        assert_eq!(fixture.registry_state()?, registry_before);
    }
    Ok(())
}

#[test]
fn public_runner_family_rejects_unbound_dockerfile_context_sources() -> Result<(), Box<dyn Error>> {
    for case in context_cases() {
        let fixture = Fixture::new_for(case.profile)?;
        let mut dockerfile = source_bytes(case.dockerfile)?;
        case.mutate(&mut dockerfile)?;
        let registry_before = fixture.preseed_registry_state()?;
        fixture.replace_authenticated_source(case.dockerfile, &dockerfile)?;
        let output = fixture.invoke()?;
        assert!(
            !output.status.success(),
            "accepted Dockerfile context mutation {}",
            case.name
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("image_publication_source_context"),
            "wrong rejection for {}: {}",
            case.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(fixture.last_fixture_error()?.is_none());
        assert!(!fixture.receipt.exists());
        assert_eq!(fixture.registry_state()?, registry_before);
    }
    Ok(())
}

struct ContextCase {
    name: &'static str,
    profile: FixtureProfile,
    dockerfile: &'static str,
    mutation: ContextMutation,
}

impl ContextCase {
    fn mutate(&self, bytes: &mut Vec<u8>) -> Result<(), Box<dyn Error>> {
        let mut text = String::from_utf8(bytes.clone())?;
        match self.mutation {
            ContextMutation::Unknown => text.push_str("\nCOPY untracked-source /tmp/untracked\n"),
            ContextMutation::Missing => {
                let line = "COPY entrypoint.sh /usr/local/bin/velnor-runner-entrypoint\n";
                if !text.contains(line) {
                    return Err("Runner Dockerfile input was not found".into());
                }
                text = text.replacen(line, "", 1);
            }
            ContextMutation::Variable => {
                text.push_str("\nCOPY ${UNTRACKED_SOURCE} /tmp/untracked\n");
            }
            ContextMutation::Glob => text.push_str("\nCOPY *.sh /tmp/untracked\n"),
            ContextMutation::Directory => {
                text.push_str("\nCOPY scripts/ /tmp/untracked/\n");
            }
        }
        *bytes = text.into_bytes();
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum ContextMutation {
    Unknown,
    Missing,
    Variable,
    Glob,
    Directory,
}

fn context_cases() -> [ContextCase; 5] {
    [
        ContextCase {
            name: "unknown COPY source",
            profile: FixtureProfile::Runner,
            dockerfile: "images/runner/ubuntu-26.04/Dockerfile",
            mutation: ContextMutation::Unknown,
        },
        ContextCase {
            name: "missing known COPY source",
            profile: FixtureProfile::Runner,
            dockerfile: "images/runner/ubuntu-26.04/Dockerfile",
            mutation: ContextMutation::Missing,
        },
        ContextCase {
            name: "variable COPY source",
            profile: FixtureProfile::Dind,
            dockerfile: "images/dind/Dockerfile",
            mutation: ContextMutation::Variable,
        },
        ContextCase {
            name: "glob COPY source",
            profile: FixtureProfile::Runner,
            dockerfile: "images/runner/ubuntu-26.04/Dockerfile",
            mutation: ContextMutation::Glob,
        },
        ContextCase {
            name: "directory COPY source",
            profile: FixtureProfile::Dind,
            dockerfile: "images/dind/Dockerfile",
            mutation: ContextMutation::Directory,
        },
    ]
}

fn runner_profiles() -> [FixtureProfile; 3] {
    [
        FixtureProfile::Runner,
        FixtureProfile::Dind,
        FixtureProfile::ResourceProbe,
    ]
}

fn source_bytes(path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Ok(fs::read(root.join(path))?)
}
