// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::PathBuf};

use super::{
    nonrust_tools_fixture::{Fixture, copy_tree},
    nonrust_tools_roundtrip::{RenderedCommand, run_named},
    tools_cache_verify_hook::{Sandbox, expand_shell, pair},
};

#[test]
fn genuine_producer_cas_lookup_is_bounded_and_refuses_invalid_maps() -> Result<(), Box<dyn Error>> {
    let (fixture, _replay) = Fixture::load()?;
    let source = Sandbox::new()?;
    fixture.provision(&source)?;
    fixture.install(&source, "shellcheck", "0.11.0")?;
    let archive = fixture
        .archives
        .iter()
        .find(|archive| archive.tool == "shellcheck")
        .ok_or("missing ShellCheck")?;
    let mapping = fixture
        .cache
        .join(format!("downloads-cas/pins/sha256/{}.json", archive.sha256));
    let map: serde_json::Value = serde_json::from_slice(&fs::read(&mapping)?)?;
    let key = map["blake3"].as_str().ok_or("missing CAS key")?;
    let artifact = fixture
        .cache
        .join(format!("downloads-cas/blobs/{key}/artifact"));
    let repo = super::workflow_configuration_fixture::make_repo(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[[native_checks]]\nid = \"tap-tree\"\noperation = \"tap_tree\"\n[native_checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n",
    )?;
    fs::remove_file(repo.path().join("Cargo.toml"))?;
    fs::remove_file(repo.path().join("src/lib.rs"))?;
    let prepared = velnor_actions_orchestrator::prepare(repo.path())?;
    let rendered = velnor_actions_orchestrator::render_staged_tree(&prepared)?;
    let step = run_named(
        rendered
            .get(".github/actions/velnor-tools-cache-verify/action.yml")
            .ok_or("no verify action")?,
        "Re-verify restored Mise installs",
    )?;
    let context = Cas {
        fixture: &fixture,
        source: &source,
        step,
        sha256: &archive.sha256,
        mapping,
        artifact,
        key,
    };
    for case in [
        "valid",
        "unrelated-huge",
        "malformed",
        "duplicate",
        "trailing",
        "escaping",
        "map-link",
        "directory-link",
        "artifact-link",
        "oversize-map",
        "oversize-artifact",
        "missing",
        "forged",
        "64",
        "65",
        "overbyte",
        "timeout",
    ] {
        context.verify_case(case)?;
    }
    Ok(())
}

struct Cas<'a> {
    fixture: &'a Fixture,
    source: &'a Sandbox,
    step: RenderedCommand,
    sha256: &'a str,
    mapping: PathBuf,
    artifact: PathBuf,
    key: &'a str,
}

impl Cas<'_> {
    fn verify_case(&self, case: &str) -> Result<(), Box<dyn Error>> {
        let runner = Sandbox::new()?;
        copy_tree(
            &self.source.mise_root().join("installs/shellcheck"),
            &runner.mise_root().join("installs/shellcheck"),
        )?;
        let cache = runner
            .home
            .path()
            .join(self.fixture.cache.strip_prefix(&self.fixture.home)?);
        let cas = cache.join("downloads-cas");
        let destination = cas.join(format!("pins/sha256/{}.json", self.sha256));
        let blob = cas.join(format!("blobs/{}", self.key));
        fs::create_dir_all(destination.parent().ok_or("map parent")?)?;
        fs::create_dir_all(&blob)?;
        fs::copy(&self.mapping, &destination)?;
        fs::copy(&self.artifact, blob.join("artifact"))?;
        mutate(case, &cas, &destination, self.key)?;
        let mut environment = self.fixture.environment(&runner);
        environment.extend(
            self.step
                .env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        environment.push(pair("HTTPS_PROXY", "http://127.0.0.1:1"));
        let output = if case == "timeout" {
            super::nonrust_cas_timeout::verify(
                &runner,
                &self.step,
                &environment,
                &blob.join("artifact"),
            )?
        } else if case == "oversize-artifact" {
            super::nonrust_cas_timeout::forbid_hash(
                &runner,
                &self.step,
                &environment,
                &blob.join("artifact"),
            )?
        } else {
            runner.run_with_output(
                &expand_shell(&self.step.shell),
                &self.step.run,
                &environment,
            )?
        };
        let accepted = matches!(case, "valid" | "unrelated-huge" | "missing" | "64");
        assert_eq!(
            output.code,
            Some(i32::from(!accepted)),
            "{case}: {}\n{}",
            output.stdout,
            output.stderr
        );
        assert_eq!(
            runner
                .mise_root()
                .join("installs/shellcheck/0.11.0")
                .exists(),
            accepted,
            "{case}"
        );
        assert!(
            output.github_output.contains("origin-fetches=0\n"),
            "{case}: {}",
            output.github_output
        );
        eprintln!(
            "cas_case={case} accepted={accepted} receipt={:?}",
            output.github_output
        );
        Ok(())
    }
}

fn sparse(path: &std::path::Path, bytes: u64) -> Result<(), Box<dyn Error>> {
    fs::File::create(path)?.set_len(bytes)?;
    Ok(())
}

fn mutate(
    case: &str,
    cas: &std::path::Path,
    mapping: &std::path::Path,
    key: &str,
) -> Result<(), Box<dyn Error>> {
    let blob = cas.join(format!("blobs/{key}"));
    match case {
        "valid" => {}
        "malformed" => fs::write(mapping, format!("garbage {{\"blake3\":\"{key}\"}}"))?,
        "duplicate" => fs::write(
            mapping,
            format!(
                "{{\"blake3\":\"{key}\",\"blake3\":\"{key}\",\"effective_filename\":null,\"urls\":[]}}"
            ),
        )?,
        "trailing" => fs::write(
            mapping,
            format!("{} trailing", fs::read_to_string(mapping)?),
        )?,
        "escaping" => fs::write(
            mapping,
            format!("{{\"blake3\":\"{key}\",\"effective_filename\":\"\\u0041\",\"urls\":[]}}"),
        )?,
        "map-link" => link(mapping)?,
        "directory-link" => link(&blob)?,
        "artifact-link" => link(&blob.join("artifact"))?,
        "oversize-map" => {
            let mut text = fs::read_to_string(mapping)?;
            text.push_str(&" ".repeat(65_537 - text.len()));
            fs::write(mapping, text)?;
        }
        "oversize-artifact" => sparse(&blob.join("artifact"), 1_610_612_737)?,
        "missing" => fs::remove_file(mapping)?,
        "timeout" => {
            fs::remove_file(mapping)?;
            sparse(&blob.join("artifact"), 1_610_612_736)?;
        }
        "forged" => {
            fs::write(
                mapping,
                format!(
                    "{{\"blake3\":\"{}\",\"effective_filename\":null,\"urls\":[]}}",
                    "f".repeat(64)
                ),
            )?;
            let forged = cas.join(format!("blobs/{}", "f".repeat(64)));
            fs::create_dir(&forged)?;
            fs::write(forged.join("artifact"), "wrong bytes")?;
        }
        "unrelated-huge" => {
            let path = unrelated(cas, 1)?;
            sparse(&path, 4_000_000_000_000)?;
        }
        "64" | "65" => {
            fs::remove_file(mapping)?;
            for index in 1..if case == "64" { 64 } else { 65 } {
                fs::write(unrelated(cas, index)?, "unrelated")?;
            }
        }
        "overbyte" => {
            fs::remove_file(mapping)?;
            for index in 1..=2 {
                sparse(&unrelated(cas, index)?, 900_000_000)?;
            }
        }
        _ => return Err("unknown case".into()),
    }
    Ok(())
}

fn unrelated(cas: &std::path::Path, index: u32) -> Result<PathBuf, Box<dyn Error>> {
    let dir = cas.join(format!("blobs/{index:064x}"));
    fs::create_dir(&dir)?;
    Ok(dir.join("artifact"))
}

fn link(path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let target = path.with_extension("real");
    fs::rename(path, &target)?;
    std::os::unix::fs::symlink(target, path)?;
    Ok(())
}
