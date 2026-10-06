//! Compiled distribution descriptor and executable identity regressions.

use std::error::Error;

use serde_json::json;

use super::{p12_delivery::edit_inventory, p12_harness as harness};

#[test]
fn mise_constructor_must_preserve_host_selector_and_provisioning_identity()
-> Result<(), Box<dyn Error>> {
    const DESCRIPTOR: &str = "crates/velnor-actions-mise/src/catalog_qualification_records.rs";
    for (old, new) in [
        (
            "        host,\n        selector:",
            "        host: DistributionHost::MacosArm64,\n        selector:",
        ),
        (
            "selector: asset.selector()",
            "selector: \"github:unrelated/tool@2026.10.0\"",
        ),
        (
            "selection_version: asset.version()",
            "selection_version: \"2026.9.18\"",
        ),
        (
            "installed_binary_relative_path: None",
            "installed_binary_relative_path: Some(\"bin/mise\")",
        ),
        ("launch_entries: &[]", "launch_entries: &[unqualified]"),
        ("source_lineage: &[]", "source_lineage: &[unqualified]"),
        ("install_plan: None", "install_plan: Some(unqualified)"),
        (
            "provisioning_mode: ProvisioningMode::Official",
            "provisioning_mode: ProvisioningMode::MiseNoMisercExclusiveConfig",
        ),
    ] {
        let fixture = harness::passing("p12-mise-constructor-identity")?;
        harness::mutate(&fixture.dir, DESCRIPTOR, old, new)?;
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(
            &run,
            "unsupported official qualification constructor identity",
        );
        assert!(!run.stderr.contains("Traceback"), "{}", run.stderr);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn mise_compiled_container_and_member_must_match_the_host_artifact() -> Result<(), Box<dyn Error>> {
    const DESCRIPTOR: &str = "crates/velnor-actions-mise/src/catalog_source_build_bootstrap.rs";
    for mutation in ["container", "member"] {
        let fixture = harness::passing("p12-mise-container-member")?;
        let path = fixture.dir.join(DESCRIPTOR);
        let source = std::fs::read_to_string(&path)?;
        let (old, new) = if mutation == "container" {
            (
                "SourceBuildBootstrapFormat::TarGzip,",
                "SourceBuildBootstrapFormat::Binary,",
            )
        } else {
            ("\"mise/bin/mise\",", "\"mise/bin/other\",")
        };
        assert_eq!(source.matches(old).count(), 2, "fixture tar hosts");
        std::fs::write(path, source.replacen(old, new, 1))?;
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(
            &run,
            "unsupported compiled qualification container or member",
        );
        assert!(!run.stderr.contains("Traceback"), "{}", run.stderr);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn mise_descriptor_requires_live_tuples_and_unchanged_constructor_fields()
-> Result<(), Box<dyn Error>> {
    const DESCRIPTOR: &str = "crates/velnor-actions-mise/src/catalog_source_build_bootstrap.rs";
    for mutation in ["constructor", "comment-only-host"] {
        let fixture = harness::passing("p12-mise-descriptor-parser")?;
        if mutation == "constructor" {
            harness::mutate(
                &fixture.dir,
                DESCRIPTOR,
                "        binary_sha256,\n",
                "        binary_sha256: archive_sha256,\n",
            )?;
        } else {
            let path = fixture.dir.join(DESCRIPTOR);
            let source = std::fs::read_to_string(&path)?;
            let start = source
                .find("        SourceBuildBootstrapHost::LinuxArm64 => (")
                .ok_or("fixture arm64 tuple")?;
            let end = start
                + source[start..]
                    .find("        ),")
                    .ok_or("fixture arm64 tuple end")?
                + "        ),".len();
            let tuple = &source[start..end];
            std::fs::write(path, source.replacen(tuple, &format!("/*{tuple}*/"), 1))?;
        }
        let run = harness::run_script(&fixture.dir, &[])?;
        let failure = if mutation == "constructor" {
            "tuple fields must forward unchanged"
        } else {
            "missing, duplicate, or unknown qualified host"
        };
        harness::assert_fail(&run, failure);
        assert!(!run.stderr.contains("Traceback"), "{}", run.stderr);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn mise_bootstrap_authority_requires_the_exact_live_source_chain() -> Result<(), Box<dyn Error>> {
    const SOURCE: &str = "crates/velnor-actions-mise/src/catalog_source_build_bootstrap.rs";
    for (path, old, new, failure) in [
        (
            "crates/velnor-actions-mise/src/catalog.rs",
            "#[path = \"catalog_source_build_bootstrap.rs\"]",
            "#[path = \"unbound_bootstrap.rs\"]",
            "native module binding",
        ),
        (
            SOURCE,
            "SourceBuildBootstrapTool::Mise => mise(host)",
            "SourceBuildBootstrapTool::Mise => mbx(host)",
            "source bootstrap dispatch",
        ),
        (
            SOURCE,
            "        self.binary_sha256\n",
            "        self.archive_sha256\n",
            "source bootstrap accessor identity",
        ),
        (
            SOURCE,
            "        SourceBuildBootstrapHost::LinuxArm64 => (",
            "        SourceBuildBootstrapHost::LinuxAmd64 => (",
            "missing, duplicate, or unknown qualified host",
        ),
        (
            "crates/velnor-actions-mise/src/catalog_qualification_records.rs",
            "let asset = source_build_bootstrap::official(source_tool, source_host);",
            "let asset = source_build_bootstrap::official(source_tool, SourceBuildBootstrapHost::LinuxAmd64);",
            "official qualification source adapter",
        ),
    ] {
        let fixture = harness::passing("p12-mise-source-chain")?;
        if path == SOURCE && old.contains("LinuxArm64") {
            let source = std::fs::read_to_string(fixture.dir.join(path))?;
            std::fs::write(fixture.dir.join(path), source.replacen(old, new, 1))?;
        } else {
            harness::mutate(&fixture.dir, path, old, new)?;
        }
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(&run, failure);
        assert!(!run.stderr.contains("Traceback"), "{}", run.stderr);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn mise_bootstrap_parser_rejects_unrecognized_executable_source() -> Result<(), Box<dyn Error>> {
    const SOURCE: &str = "crates/velnor-actions-mise/src/catalog_source_build_bootstrap.rs";
    const ADAPTER: &str = "crates/velnor-actions-mise/src/catalog_qualification_records.rs";
    for (path, old, new, failure) in [
        (
            ADAPTER,
            "    .validate()\n",
            "    .validate().map(|asset| asset)\n",
            "official qualification constructor shape",
        ),
        (
            ADAPTER,
            "use super::super::source_build_bootstrap::{",
            "use super::super::unbound_bootstrap::{",
            "official qualification source binding",
        ),
        (
            SOURCE,
            "const fn mise(host:",
            "#[cfg(any())]\nconst fn mise(host:",
            "source bootstrap authority binding",
        ),
        (
            SOURCE,
            "    let (asset_url, archive_sha256, binary_sha256, asset_format, binary_member) = match host {",
            "    let (asset_url, archive_sha256, binary_sha256, asset_format, binary_member) = match host {\n        SourceBuildBootstrapHost::LinuxAmd64 if true => (\"https://bad.example/mise\", \"bad\", \"bad\", SourceBuildBootstrapFormat::Binary, \"\"),",
            "unsupported qualified host match arm",
        ),
    ] {
        let fixture = harness::passing("p12-mise-executable-source")?;
        harness::mutate(&fixture.dir, path, old, new)?;
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(&run, failure);
        assert!(!run.stderr.contains("Traceback"), "{}", run.stderr);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn mise_archive_digest_must_match_the_compiled_descriptor() -> Result<(), Box<dyn Error>> {
    for mutation in ["inventory", "descriptor"] {
        let fixture = harness::passing("p12-mise-archive-descriptor")?;
        if mutation == "inventory" {
            edit_inventory(&fixture.dir, |inventory| {
                let mise = inventory["tools"]
                    .as_array_mut()
                    .expect("fixture tools")
                    .iter_mut()
                    .find(|tool| tool["name"] == "mise")
                    .expect("fixture Mise");
                mise["native_artifacts"][0]["archive_sha256"] = json!("0".repeat(64));
            })?;
        } else {
            harness::mutate(
                &fixture.dir,
                "crates/velnor-actions-mise/src/catalog_source_build_bootstrap.rs",
                "107c5e46693cdfeb1fdec91717078b298d6fcc9ebbd14f8333917cfe37965138",
                &"0".repeat(64),
            )?;
        }
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(&run, "archive digest differs from compiled qualification");
        assert!(harness::has_fail(&run, "mise"), "{}", run.stdout);
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn mise_native_artifacts_require_complete_unambiguous_binary_evidence() -> Result<(), Box<dyn Error>>
{
    for mutation in [
        "duplicate",
        "missing",
        "unqualified",
        "wrong-qualification",
        "archive-confusion",
    ] {
        let fixture = harness::passing("p12-mise-native-evidence")?;
        edit_inventory(&fixture.dir, |inventory| {
            let mise = inventory["tools"]
                .as_array_mut()
                .expect("fixture tools")
                .iter_mut()
                .find(|tool| tool["name"] == "mise")
                .expect("fixture Mise");
            let artifacts = mise["native_artifacts"]
                .as_array_mut()
                .expect("fixture native artifacts");
            match mutation {
                "duplicate" => artifacts.push(artifacts[0].clone()),
                "missing" => {
                    assert!(artifacts.pop().is_some());
                }
                "unqualified" => {
                    assert!(
                        artifacts[0]
                            .as_object_mut()
                            .expect("fixture artifact")
                            .remove("qualified_installed_binary_sha256")
                            .is_some()
                    );
                }
                "wrong-qualification" => {
                    artifacts[0]["qualified_installed_binary_sha256"] = json!("0".repeat(64));
                }
                "archive-confusion" => {
                    let archive_digest = artifacts[0]["archive_sha256"].clone();
                    assert!(
                        archive_digest.is_string(),
                        "fixture archive must carry distinct digest"
                    );
                    artifacts[0]["installed_binary_sha256"] = archive_digest.clone();
                    artifacts[0]["qualified_installed_binary_sha256"] = archive_digest;
                }
                _ => unreachable!("closed mutation table"),
            }
        })?;
        let run = harness::run_script(&fixture.dir, &[])?;
        let failure = match mutation {
            "duplicate" => "unknown or duplicate target",
            "missing" => "artifact inventory row missing",
            "archive-confusion" => "code=",
            _ => "installed binary digest has no matching qualification",
        };
        harness::assert_fail(&run, failure);
        assert!(harness::has_fail(&run, "mise"), "{}", run.stdout);
        assert!(!run.stderr.contains("Traceback"), "{}", run.stderr);
        harness::cleanup(&fixture);
    }
    Ok(())
}
