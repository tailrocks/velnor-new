// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fs,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};

use super::{Fixture, FixtureProfile, oci};

const VERSION: &str = "1.2.3";
const TAG_OBJECT_SHA: &str = "dddddddddddddddddddddddddddddddddddddddd";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromotionCase {
    Valid,
    MatchingExisting,
    WrongPublisherRun,
    WrongAnnotatedTarget,
    WrongTagObjectType,
    WrongVersionDeclaration,
    WrongImageDeclaration,
    ValidEncodedPath,
    GenericUnlabeled,
    ReservedJobRepository,
    FailedRequiredCi,
    AttestationFailure,
    ExistingConflict,
}

impl Fixture {
    pub(crate) fn new_unlabeled_promotion() -> Result<Self, Box<dyn Error>> {
        let mut fixture = Self::new_for(FixtureProfile::Runner)?;
        let image = oci::create(&fixture.source)?;
        fixture.expected_digest = image.identity.platform_manifest_digest;
        fixture.expected_inventory = image.inventory;
        Ok(fixture)
    }

    pub(crate) fn set_promotion_case(&self, case: PromotionCase) -> Result<(), Box<dyn Error>> {
        if self.profile != FixtureProfile::Runner {
            return Err("promotion fixture requires the Runner image".into());
        }
        let mut state = self.read_state()?;
        let tag = format!("runner-v{VERSION}");
        let repository = promotion_repository(case, self.profile.repository());
        let version_reference = format!("{repository}:{VERSION}");
        if matches!(
            case,
            PromotionCase::GenericUnlabeled | PromotionCase::ReservedJobRepository
        ) {
            state["crane"]["tag_reference"] = json!(format!("{repository}:sha-{}", self.source));
        }
        let source_digest = self.expected_digest.clone();
        set_promotion_source(self, &mut state, case, &source_digest)?;
        let version_digest = set_existing_version(self, &mut state, case, &source_digest)?;
        state["crane"]["promotion_tags"] = json!({version_reference.clone(): version_digest});
        state["crane"]["promotion_operations"] = json!([]);
        state["promotion"] = json!({
            "case":format!("{case:?}"),
            "tag":tag,
            "tag_object_sha":TAG_OBJECT_SHA,
            "reference_object_type":"tag",
            "annotation_target":self.source,
            "version":VERSION,
            "version_reference":version_reference,
            "attestation_failure":case == PromotionCase::AttestationFailure
        });
        state["publisher"] = publisher_run(self.profile.publisher_run_id(), &self.source, &tag);
        apply_promotion_rejection(&mut state, case)?;
        self.write_state(&state)
    }

    pub(crate) fn promotion_state(&self) -> Result<Value, Box<dyn Error>> {
        let state = self.read_state()?;
        let crane = &state["crane"];
        let promotion = &state["promotion"];
        let inventory = crane["remote"]["inventory"]
            .as_object()
            .ok_or("source registry inventory is missing")?;
        let blobs = inventory.keys().cloned().collect::<Vec<_>>();
        Ok(json!({
            "source_tag":{"reference":crane["tag_reference"],"digest":crane["tag_digest"]},
            "version_tag":{"reference":promotion["version_reference"],"digest":crane["promotion_tags"][promotion["version_reference"].as_str().ok_or("version reference missing")?]},
            "blobs":blobs,
            "operations":crane["promotion_operations"]
        }))
    }

    pub(crate) fn invoke_promotion(&self) -> Result<Output, std::io::Error> {
        match fs::remove_file(&self.error_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let home = self
            .state
            .parent()
            .ok_or_else(|| std::io::Error::other("fixture state has no parent directory"))?;
        let path = self
            .mise_bin
            .parent()
            .ok_or_else(|| std::io::Error::other("fixture mise binary has no parent directory"))?;
        let tag = format!("runner-v{VERSION}");
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
        command
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PATH", path)
            .env("HOME", home)
            .env("GH_TOKEN", "fixture-source-token")
            .env("DOCKER_CONFIG", &self.docker_config)
            .env("GITHUB_EVENT_NAME", "push")
            .env("GITHUB_REF_NAME", &tag)
            .env("GITHUB_REF", format!("refs/tags/{tag}"))
            .env("GITHUB_SHA", &self.source)
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_RUN_ID", self.profile.publisher_run_id().to_string())
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env(
                "GITHUB_WORKFLOW_REF",
                format!("tailrocks/velnor-new/.github/workflows/publish.yml@refs/tags/{tag}"),
            )
            .env("GITHUB_WORKFLOW_SHA", &self.source)
            .args([
                "image-maintenance",
                "promote-image",
                "--image-id",
                "runner",
                "--source-commit",
                &self.source,
            ]);
        command.output()
    }
}

fn set_promotion_source(
    fixture: &Fixture,
    state: &mut Value,
    case: PromotionCase,
    source_digest: &str,
) -> Result<(), Box<dyn Error>> {
    let repository = promotion_repository(case, fixture.profile.repository());
    let config = promotion_config("runner", repository)?;
    state["source_files"][".velnor/config.toml"] =
        json!(super::records::encode_base64(config.as_bytes()));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = fs::read_to_string(root.join("crates/velnor-runner/Cargo.toml"))?
        .replace("version = \"0.1.0\"", "version = \"1.2.3\"");
    state["source_files"]["crates/velnor-runner/Cargo.toml"] =
        json!(super::records::encode_base64(manifest.as_bytes()));
    if case == PromotionCase::ValidEncodedPath {
        let config = promotion_config_with_version_file(
            "runner",
            "ghcr.io/tailrocks/velnor-runner",
            "crates/a+@b/Cargo.toml",
        )?;
        state["source_files"][".velnor/config.toml"] =
            json!(super::records::encode_base64(config.as_bytes()));
        state["source_files"]["crates/a+@b/Cargo.toml"] = json!(super::records::encode_base64(
            b"[package]\nname = \"fixture\"\nversion = \"1.2.3\"\n"
        ));
    }
    let mut inventory = serde_json::Map::new();
    for (path, bytes) in &fixture.expected_inventory {
        let name = path
            .rsplit('/')
            .next()
            .ok_or("source blob path has no digest")?;
        inventory.insert(
            path.clone(),
            json!({
                "digest":format!("sha256:{name}"),
                "size":bytes.len(),
                "bytes":bytes
            }),
        );
    }
    let _source_reference = state["crane"]["tag_reference"]
        .as_str()
        .ok_or("source tag reference is missing")?;
    state["crane"]["tag_digest"] = json!(source_digest);
    state["crane"]["manifest_reference"] = json!(format!("{repository}@{source_digest}"));
    state["crane"]["remote"] = json!({"manifest_digest":source_digest,"inventory":inventory});
    Ok(())
}

fn promotion_repository(case: PromotionCase, default: &'static str) -> &'static str {
    match case {
        PromotionCase::GenericUnlabeled => "ghcr.io/tailrocks/generic",
        PromotionCase::ReservedJobRepository => "ghcr.io/tailrocks/velnor-job",
        _ => default,
    }
}

fn set_existing_version(
    fixture: &Fixture,
    state: &mut Value,
    case: PromotionCase,
    source_digest: &str,
) -> Result<Value, Box<dyn Error>> {
    match case {
        PromotionCase::MatchingExisting => Ok(json!(source_digest)),
        PromotionCase::ExistingConflict => {
            let foreign = oci::create_for(FixtureProfile::Dind, &fixture.source)?;
            let digest = foreign.identity.platform_manifest_digest.clone();
            let inventory = &mut state["crane"]["remote"]["inventory"];
            for (path, bytes) in foreign.inventory {
                let name = path
                    .rsplit('/')
                    .next()
                    .ok_or("foreign blob path has no digest")?;
                inventory[path] = json!({
                    "digest":format!("sha256:{name}"),
                    "size":bytes.len(),
                    "bytes":bytes
                });
            }
            Ok(json!(digest))
        }
        _ => Ok(Value::Null),
    }
}

fn apply_promotion_rejection(state: &mut Value, case: PromotionCase) -> Result<(), Box<dyn Error>> {
    match case {
        PromotionCase::WrongPublisherRun => {
            state["publisher"]["path"] = json!(".github/workflows/untrusted.yml");
        }
        PromotionCase::WrongAnnotatedTarget => {
            state["promotion"]["annotation_target"] = json!("c".repeat(40));
        }
        PromotionCase::WrongTagObjectType => {
            state["promotion"]["reference_object_type"] = json!("commit");
        }
        PromotionCase::WrongVersionDeclaration => {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let text = fs::read_to_string(root.join("crates/velnor-runner/Cargo.toml"))?
                .replace("version = \"0.1.0\"", "version = \"9.8.7\"");
            state["source_files"]["crates/velnor-runner/Cargo.toml"] =
                json!(super::records::encode_base64(text.as_bytes()));
        }
        PromotionCase::WrongImageDeclaration => {
            let config = promotion_config("unlisted-runner", "ghcr.io/tailrocks/velnor-runner")?;
            state["source_files"][".velnor/config.toml"] =
                json!(super::records::encode_base64(config.as_bytes()));
        }
        PromotionCase::FailedRequiredCi => {
            state["ci_jobs"][1]["conclusion"] = json!("failure");
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn promotion_config(id: &str, repository: &str) -> Result<String, Box<dyn Error>> {
    promotion_config_with_version_file(id, repository, "crates/velnor-runner/Cargo.toml")
}

fn promotion_config_with_version_file(
    id: &str,
    repository: &str,
    version_file: &str,
) -> Result<String, Box<dyn Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(root.join(".velnor/config.toml"))?)?;
    let mut image = toml::map::Map::new();
    image.insert("id".into(), toml::Value::String(id.to_owned()));
    image.insert(
        "repository".into(),
        toml::Value::String(repository.to_owned()),
    );
    image.insert(
        "dockerfile".into(),
        toml::Value::String("images/runner/ubuntu-26.04/Dockerfile".into()),
    );
    image.insert(
        "platforms".into(),
        toml::Value::Array(vec![toml::Value::String("linux_amd64".into())]),
    );
    image.insert(
        "version_file".into(),
        toml::Value::String(version_file.into()),
    );
    config
        .as_table_mut()
        .ok_or("promotion config is not a TOML table")?
        .insert(
            "images".into(),
            toml::Value::Array(vec![toml::Value::Table(image)]),
        );
    Ok(toml::to_string(&config)?)
}

fn publisher_run(id: u64, source: &str, tag: &str) -> Value {
    json!({"id":id,"run_attempt":1,"event":"push","path":".github/workflows/publish.yml","head_branch":tag,"head_sha":source,
        "head_repository":{"id":1_390_620_900,"full_name":"tailrocks/velnor-new"},"status":"in_progress","conclusion":null})
}
