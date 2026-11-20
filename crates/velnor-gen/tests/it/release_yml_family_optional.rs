// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::GenerateError;
use velnor_release::{CheckProblem, FamilyError, ReleaseError};

use crate::release_family_fixture::{generate, refused};
use crate::release_fixture::steps;
use crate::release_yml_binaries::{job, runs};
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

const OPTIONAL_HELPER: &str = "0.1.109";
const HEAD: &str = "family_tag = \"tools-v{{ version }}\"\nroots = [\".\", \"tools\"]\nchecksums_file = \"BINARY-SHA256SUMS\"\ncommit_file = true\nattest = true\n";
const BINARY: &str = "[[binaries]]\npackage = \"app\"\nsmoke = true\nplatforms = [\"linux-x64\", \"linux-arm64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n[[binaries]]\npackage = \"tools\"\nsmoke = true\nplatforms = [\"linux-x64\", \"linux-arm64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n";
const APP_MACOS: &str = "app-aarch64-apple-darwin.tar.gz";
const TOOLS_MACOS: &str = "tools-aarch64-apple-darwin.tar.gz";

fn config() -> String {
    format!("{HEAD}{BINARY}")
}

fn rendered() -> Result<Node, Box<dyn Error>> {
    Ok(generate(OPTIONAL_HELPER, &config())?.1)
}

fn needs(job: &Node) -> Vec<&str> {
    job.get("needs").map_or_else(Vec::new, Node::texts)
}

fn condition(job: &Node) -> &str {
    job.get("if").and_then(Node::text).unwrap_or("")
}

fn step_named<'a>(job: &'a Node, name: &str) -> Result<&'a Node, Box<dyn Error>> {
    job.get("steps")
        .map_or(&[][..], Node::items)
        .iter()
        .find(|step| step.get("name").and_then(Node::text) == Some(name))
        .ok_or_else(|| format!("no step {name}").into())
}

fn continues(step: &Node) -> bool {
    step.get("continue-on-error").and_then(Node::text) == Some("true")
}

#[test]
fn an_optional_build_cannot_fail_the_run_and_a_required_one_still_can() -> TestResult {
    let tree = rendered()?;
    for id in ["build-app-macos-arm64", "build-tools-macos-arm64"] {
        let build = job(&tree, id)?;
        assert_eq!(build.get("runs-on").and_then(Node::text), Some("macos-15"));
        assert_eq!(
            build.get("continue-on-error").and_then(Node::text),
            Some("true"),
            "{id}"
        );
    }
    for id in ["build-app-linux-x64", "build-tools-linux-arm64"] {
        assert!(job(&tree, id)?.get("continue-on-error").is_none(), "{id}");
    }
    Ok(())
}

#[test]
fn the_checksums_wait_for_every_build_and_need_every_required_one_to_succeed() -> TestResult {
    let tree = rendered()?;
    let sums = job(&tree, "sums-family")?;
    assert!(needs(sums).contains(&"build-tools-macos-arm64"));
    let condition = condition(sums);
    assert!(condition.starts_with("${{ always() && "), "{condition}");
    for required in [
        "gate",
        "family",
        "build-app-linux-x64",
        "build-app-linux-arm64",
        "build-tools-linux-x64",
        "build-tools-linux-arm64",
    ] {
        assert!(
            condition.contains(&format!("needs.{required}.result == 'success'")),
            "{required}: {condition}"
        );
    }
    assert!(!condition.contains("macos"), "{condition}");
    assert!(condition.contains("needs.gate.outputs.ready == 'true'"));
    Ok(())
}

#[test]
fn the_publish_chain_stays_behind_the_checksums_and_needs_no_optional_build() -> TestResult {
    let tree = rendered()?;
    for id in ["verify-family", "publish-family"] {
        let later = job(&tree, id)?;
        assert!(!condition(later).contains("always()"), "{id}");
        assert!(
            needs(later).iter().all(|need| !need.contains("macos")),
            "{id}"
        );
    }
    assert!(needs(job(&tree, "publish-family")?).contains(&"verify-family"));
    assert!(needs(job(&tree, "verify-family")?).contains(&"sums-family"));
    Ok(())
}

#[test]
fn the_downloads_of_an_optional_artifact_may_fail_and_the_required_ones_may_not() -> TestResult {
    let tree = rendered()?;
    for id in ["sums-family", "verify-family", "publish-family"] {
        let later = job(&tree, id)?;
        for name in ["app-macos-arm64", "tools-macos-arm64"] {
            assert!(
                continues(step_named(later, &format!("Download {name}"))?),
                "{id} {name}"
            );
        }
        for name in ["app-linux-x64", "tools-linux-arm64"] {
            assert!(
                !continues(step_named(later, &format!("Download {name}"))?),
                "{id} {name}"
            );
        }
    }
    Ok(())
}

#[test]
fn the_checksums_list_an_optional_file_only_when_it_exists() -> TestResult {
    let tree = rendered()?;
    let sums = job(&tree, "sums-family")?;
    let required: Vec<&str> = runs(sums)
        .into_iter()
        .filter(|run| run.contains(" > SHA256SUMS"))
        .collect();
    assert_eq!(required.len(), 2);
    for run in &required {
        assert!(
            run.contains("app-x86_64-unknown-linux-musl.tar.gz"),
            "{run}"
        );
        assert!(!run.contains("apple-darwin"), "{run}");
    }
    for file in [APP_MACOS, TOOLS_MACOS] {
        let step = step_named(sums, &format!("Add {file} to SHA256SUMS"))?;
        let text = step.get("if").and_then(Node::text).unwrap_or("");
        assert!(
            text.contains(&format!("hashFiles('dist/{file}') != ''")),
            "{text}"
        );
        let run = step.get("run").and_then(Node::text).unwrap_or("");
        assert!(run.ends_with(&format!("{file} >> SHA256SUMS")), "{run}");
    }
    Ok(())
}

#[test]
fn the_member_digests_unpack_and_list_an_optional_archive_only_when_it_exists() -> TestResult {
    let tree = rendered()?;
    let sums = job(&tree, "sums-family")?;
    let unpack = step_named(sums, &format!("Unpack {APP_MACOS}"))?;
    assert!(
        unpack
            .get("if")
            .and_then(Node::text)
            .is_some_and(|text| text.contains(&format!("hashFiles('dist/{APP_MACOS}') != ''")))
    );
    let required = step_named(sums, "Write BINARY-SHA256SUMS")?;
    let run = required.get("run").and_then(Node::text).unwrap_or("");
    assert!(run.contains("linux-musl.tar.gz/app"), "{run}");
    assert!(!run.contains("apple-darwin"), "{run}");
    let added = step_named(sums, &format!("Add {APP_MACOS} to BINARY-SHA256SUMS"))?;
    let run = added.get("run").and_then(Node::text).unwrap_or("");
    assert!(run.contains(">> ../dist/BINARY-SHA256SUMS"), "{run}");
    let unpack_linux = step_named(sums, "Unpack app-x86_64-unknown-linux-musl.tar.gz")?;
    assert!(unpack_linux.get("if").is_none());
    Ok(())
}

#[test]
fn only_the_attestation_of_an_existing_optional_file_is_verified() -> TestResult {
    let tree = rendered()?;
    let verify = job(&tree, "verify-family")?;
    for file in [APP_MACOS, TOOLS_MACOS] {
        let step = step_named(verify, &format!("Verify the build provenance of {file}"))?;
        let text = step.get("if").and_then(Node::text).unwrap_or("");
        assert!(
            text.contains(&format!("hashFiles('dist/{file}') != ''")),
            "{text}"
        );
    }
    let required = step_named(
        verify,
        "Verify the build provenance of app-x86_64-unknown-linux-musl.tar.gz",
    )?;
    assert!(required.get("if").is_none());
    Ok(())
}

#[test]
fn the_publish_names_an_optional_file_as_optional() -> TestResult {
    let tree = rendered()?;
    let command = runs(job(&tree, "publish-family")?)
        .into_iter()
        .find(|run| run.starts_with("velnor-ci release publish"))
        .ok_or("no publish")?;
    assert!(
        command.contains(&format!("--optional-asset={APP_MACOS}")),
        "{command}"
    );
    assert!(
        command.contains(&format!("--optional-asset={TOOLS_MACOS}")),
        "{command}"
    );
    let positional: Vec<&str> = command
        .split(' ')
        .filter(|word| word.ends_with(".tar.gz") && !word.starts_with("--"))
        .collect();
    assert!(positional.contains(&"app-x86_64-unknown-linux-musl.tar.gz"));
    assert!(!positional.iter().any(|word| word.contains("apple-darwin")));
    Ok(())
}

#[test]
fn no_run_step_of_the_release_holds_shell_logic_for_an_optional_file() -> TestResult {
    let tree = rendered()?;
    for id in ["sums-family", "verify-family", "publish-family"] {
        for run in runs(job(&tree, id)?) {
            for logic in ["$(", "; ", "&& ", " | ", "for ", "if "] {
                assert!(
                    !run.contains(logic) || run.contains("RUNNER_TEMP"),
                    "{id}: {run}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_release_without_optional_platforms_renders_none_of_it() -> TestResult {
    let plain = config().replace("optional_platforms = [\"macos-arm64\"]\n", "");
    let (text, tree) = generate(OPTIONAL_HELPER, &plain)?;
    assert!(!text.contains("continue-on-error"));
    assert!(!text.contains("always()"));
    assert!(!text.contains("--optional-asset"));
    assert!(!tree.has_key_anywhere("continue-on-error"));
    Ok(())
}

fn refusal_at(helper: &str, config: &str) -> Result<GenerateError, Box<dyn Error>> {
    match generate(helper, config) {
        Ok(_) => Err("the configuration was accepted".into()),
        Err(error) => match error.downcast::<GenerateError>() {
            Ok(error) => Ok(*error),
            Err(other) => Err(other),
        },
    }
}

#[test]
fn optional_platforms_need_a_helper_that_publishes_optional_files() -> TestResult {
    let error = refusal_at("0.1.108", &config())?;
    assert!(
        matches!(
            error,
            GenerateError::ConfigKeyHelperTooOld {
                key: "binaries.optional_platforms",
                ..
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("pin-helper 0.1.109"), "{error}");
    generate(OPTIONAL_HELPER, &config())?;
    Ok(())
}

#[test]
fn a_publish_check_cannot_require_an_optional_artifact() -> TestResult {
    let checks = "[[checks]]\nname = \"qualify\"\nrun = \"x\"\nbefore = \"publish\"\nartifacts = [\"app-macos-arm64\"]\n";
    let error = refusal_at(OPTIONAL_HELPER, &format!("{HEAD}{BINARY}{checks}"))?;
    assert!(
        matches!(
            &error,
            GenerateError::Release(ReleaseError::InvalidCheck(name, CheckProblem::OptionalArtifact(artifact)))
                if name == "qualify" && artifact == "app-macos-arm64"
        ),
        "{error}"
    );
    Ok(())
}

#[test]
fn an_optional_build_is_verified_when_the_checksums_list_it_not_when_it_downloaded() -> TestResult {
    let tree = rendered()?;
    let verify = job(&tree, "verify-family")?;
    let name = format!("Verify the build provenance of {APP_MACOS}");
    let step = steps(verify)
        .iter()
        .find(|step| step.get("name").and_then(Node::text) == Some(name.as_str()))
        .ok_or("no provenance step for the optional build")?;
    assert!(step.get("if").is_none(), "{step:?}");
    Ok(())
}

fn family_error(config: &str) -> Result<FamilyError, Box<dyn Error>> {
    match refused(config)? {
        GenerateError::Release(ReleaseError::Family(error)) => Ok(error),
        other => Err(format!("unexpected error: {other}").into()),
    }
}

#[test]
fn an_optional_platform_must_be_built_and_a_binary_keeps_one_required_platform() -> TestResult {
    let one = |platforms: &str, optional: &str| {
        format!(
            "{HEAD}[[binaries]]\npackage = \"app\"\nplatforms = {platforms}\noptional_platforms = {optional}\n"
        )
    };
    assert_eq!(
        family_error(&one("[\"linux-x64\"]", "[\"macos-arm64\"]"))?,
        FamilyError::OptionalPlatformNotBuilt {
            package: "app".to_owned(),
            platform: "macos-arm64"
        }
    );
    assert_eq!(
        family_error(&one("[\"macos-arm64\"]", "[\"macos-arm64\"]"))?,
        FamilyError::EveryPlatformOptional {
            package: "app".to_owned()
        }
    );
    assert!(matches!(
        refused(&one("[\"linux-x64\"]", "[\"haiku\"]"))?,
        GenerateError::Release(ReleaseError::UnknownPlatform { .. })
    ));
    Ok(())
}

#[test]
fn an_optional_platform_needs_the_family_and_an_asset_name_without_a_version() -> TestResult {
    assert_eq!(
        family_error(
            "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n"
        )?,
        FamilyError::NeedsFamilyTag {
            key: "optional_platforms"
        }
    );
    let versioned = format!(
        "{HEAD}[[binaries]]\npackage = \"app\"\nasset_name = \"{{{{ bin }}}}-{{{{ version }}}}-{{{{ target }}}}\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n"
    );
    assert_eq!(
        family_error(&versioned)?,
        FamilyError::OptionalPlatformVersionedName {
            package: "app".to_owned()
        }
    );
    Ok(())
}
