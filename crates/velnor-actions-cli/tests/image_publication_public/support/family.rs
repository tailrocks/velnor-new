// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Fixture, FixtureProfile};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

pub(crate) struct FamilyFixture {
    members: [Fixture; 3],
    pub(crate) assets: PathBuf,
    pub(crate) manifest: PathBuf,
}

impl FamilyFixture {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        let members = [
            Fixture::new_for(FixtureProfile::Runner)?,
            Fixture::new_for(FixtureProfile::Dind)?,
            Fixture::new_for(FixtureProfile::ResourceProbe)?,
        ];
        let assets = members[0]
            .archive
            .parent()
            .ok_or("family assets")?
            .to_owned();
        let manifest = assets.join("RUNNER_IMAGE_MANIFEST.json");
        let fixture = Self {
            members,
            assets,
            manifest,
        };
        fixture.assemble()?;
        Ok(fixture)
    }

    fn assemble(&self) -> Result<(), Box<dyn Error>> {
        let mut state = self.members[0].read_github_state()?;
        let mut registries = serde_json::Map::new();
        for member in &self.members {
            let other = member.read_github_state()?;
            registries.insert(
                member.profile.repository().to_owned(),
                other["crane"].clone(),
            );
            for path in [&member.archive, &member.record] {
                let destination = self.assets.join(path.file_name().ok_or("family filename")?);
                if *path != destination {
                    fs::copy(path, destination)?;
                }
            }
            let name = match member.profile {
                FixtureProfile::Runner => "runner-image-provenance.json",
                FixtureProfile::Dind => "dind-image-provenance.json",
                FixtureProfile::ResourceProbe => "resource-probe-image-provenance.json",
                FixtureProfile::Job => return Err("job in runner family".into()),
            };
            let source = member.archive.parent().ok_or("family source")?.join(name);
            let target = self.assets.join(name);
            if source != target {
                fs::copy(source, target)?;
            }
        }
        state["main_sha"] = json!(self.members[0].source);
        state["registries"] = Value::Object(registries);
        state["attestation"][0]["verificationResult"]["statement"]["subject"] =
            Value::Array(self.subjects()?);
        self.members[0].update_github_state(|value| {
            *value = state;
            Ok(())
        })
    }

    fn subjects(&self) -> Result<Vec<Value>, Box<dyn Error>> {
        [
            "velnor-runner-linux-amd64.tar",
            "velnor-dind-linux-amd64.tar",
            "velnor-resource-probe-linux-amd64.tar",
            "runner-image-publication.json",
            "dind-image-publication.json",
            "resource-probe-image-publication.json",
            "runner-image-provenance.json",
            "dind-image-provenance.json",
            "resource-probe-image-provenance.json",
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ]
        .into_iter()
        .map(|name| {
            let bytes = fs::read(self.assets.join(name))?;
            Ok(json!({"name":name,"digest":{"sha256":super::hex(&Sha256::digest(bytes))}}))
        })
        .collect()
    }

    pub(crate) fn invoke(&self, verify: bool) -> Result<Output, Box<dyn Error>> {
        let first = &self.members[0];
        let mut command = self.command()?;
        command
            .args([
                "image-maintenance",
                if verify {
                    "verify-runner-publication"
                } else {
                    "publish-runner-images"
                },
            ])
            .arg("--source-commit")
            .arg(&first.source)
            .args(["--producer-run-id", &first.producer_run_id.to_string()])
            .args([
                "--producer-run-attempt",
                &first.producer_run_attempt.to_string(),
            ])
            .args(["--artifact-id", &first.artifact_id.to_string()])
            .arg("--assets-dir")
            .arg(&self.assets)
            .arg("--qualification-proof")
            .arg(first.qualification_proof.as_ref().ok_or("family proof")?)
            .arg("--private-scratch")
            .arg(&first.scratch)
            .arg("--manifest-output")
            .arg(&self.manifest);
        Ok(command.output()?)
    }

    fn command(&self) -> Result<Command, Box<dyn Error>> {
        let first = &self.members[0];
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
        command
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PATH", first.mise_bin.parent().ok_or("family bin")?)
            .env("HOME", first.state.parent().ok_or("family home")?)
            .env("GH_TOKEN", "fixture-source-token")
            .env("GITHUB_EVENT_NAME", "push")
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_REF_PROTECTED", "true")
            .env("GITHUB_SHA", &first.source)
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env(
                "GITHUB_RUN_ID",
                first.profile.publisher_run_id().to_string(),
            )
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env(
                "GITHUB_WORKFLOW_REF",
                "tailrocks/velnor-new/.github/workflows/product-release-images.yml@refs/heads/main",
            )
            .env("GITHUB_WORKFLOW_SHA", &first.source)
            .env("DOCKER_CONFIG", &first.docker_config);
        Ok(command)
    }

    pub(crate) fn state(&self) -> Result<Value, Box<dyn Error>> {
        self.members[0].read_github_state()
    }

    pub(crate) fn mutate(&self, change: impl FnOnce(&mut Value)) -> Result<(), Box<dyn Error>> {
        self.members[0].update_github_state(|state| {
            change(state);
            Ok(())
        })
    }
}
