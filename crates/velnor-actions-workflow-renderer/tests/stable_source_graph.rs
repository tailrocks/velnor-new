// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::{find_file, forbid, job_section, rendered, require};

#[test]
fn stable_qualifiers_receive_their_producers_locked_source_artifact() -> Result<(), Box<dyn Error>>
{
    let files = rendered()?;
    let workflow = find_file(&files, ".github/workflows/product-release-generator.yml")?;
    for (qualifier, producer, action) in [
        (
            "qualify-linux",
            "build-linux",
            "generator-release-qualify-linux",
        ),
        (
            "qualify-macos",
            "build-macos",
            "generator-release-qualify-macos",
        ),
        (
            "qualify-macos-intel",
            "build-macos-intel",
            "generator-release-qualify-macos-intel",
        ),
        (
            "qualify-homebrew-macos",
            "build-macos",
            "generator-release-qualify-homebrew",
        ),
    ] {
        let section = job_section(workflow, qualifier)?;
        for name in [
            "sources_id",
            "sources_digest",
            "sources_tree",
            "sources_root_lock",
            "sources_runner_lock",
            "sources_attempt",
        ] {
            require(
                section,
                &format!("needs.{producer}.outputs.{name}"),
                qualifier,
            )?;
        }
        let action = find_file(&files, &format!(".github/actions/{action}/action.yml"))?;
        for name in [
            "stable-source-admission.py",
            "download-sources",
            "stable-source-consumer.sh",
            "--source-cache",
        ] {
            require(action, name, qualifier)?;
        }
        let optional = action
            .split("  sources_id:")
            .nth(1)
            .ok_or("missing source input")?
            .split("  sources_digest:")
            .next()
            .ok_or("missing source input section")?;
        require(optional, "required: false", qualifier)?;
        forbid(action, "actions/cache", qualifier)?;
    }
    for producer in ["build-linux", "build-macos", "build-macos-intel"] {
        check_source_producer(&files, workflow, producer)?;
    }
    Ok(())
}

fn check_source_producer(
    files: &[velnor_actions_workflow_renderer::RenderedFile],
    workflow: &str,
    producer: &str,
) -> Result<(), Box<dyn Error>> {
    let section = job_section(workflow, producer)?;
    require(section, "verify-release-source", producer)?;
    require(section, "steps.build.outputs.sources_id", producer)?;
    forbid(section, "actions/cache", producer)?;
    let action_name = match producer {
        "build-linux" => "generator-release-build-linux",
        "build-macos" => "generator-release-build-macos",
        _ => "generator-release-build-macos-intel",
    };
    let action = find_file(files, &format!(".github/actions/{action_name}/action.yml"))?;
    let prepare = action
        .find("stable-source-producer.sh")
        .ok_or("missing source prep")?;
    let upload = action
        .find("id: upload-sources")
        .ok_or("missing source upload")?;
    let compile = action
        .find("name: Build velnor-actions with MBX")
        .ok_or("missing build")?;
    assert!(prepare < upload && upload < compile, "{producer}");
    require(action, "include-hidden-files", producer)?;
    require(action, "steps.upload-sources.outputs.artifact-id", producer)?;
    Ok(())
}

#[test]
fn darwin_compilers_have_bounded_cold_build_time_without_changing_linux_or_qualifiers()
-> Result<(), Box<dyn Error>> {
    let files = rendered()?;
    let workflow = find_file(&files, ".github/workflows/product-release-generator.yml")?;
    for (job, minutes) in [
        ("source-preview-build-linux", 10),
        ("source-preview-build-darwin", 30),
        ("build-linux", 10),
        ("build-macos", 30),
        ("build-macos-intel", 30),
        ("qualify-linux", 20),
        ("qualify-macos", 20),
        ("qualify-macos-intel", 20),
        ("qualify-homebrew-macos", 30),
    ] {
        let section = job_section(workflow, job)?;
        let expected = format!("    timeout-minutes: {minutes}");
        assert!(section.lines().any(|line| line == expected), "{job}");
    }
    for action in [
        "generator-release-build-macos",
        "generator-release-build-macos-intel",
    ] {
        let body = find_file(&files, &format!(".github/actions/{action}/action.yml"))?;
        let compile = body
            .split("name: Build velnor-actions with MBX")
            .nth(1)
            .ok_or("missing compiler step")?
            .split("\n    - name:")
            .next()
            .ok_or("missing compiler section")?;
        forbid(compile, "timeout-minutes:", action)?;
    }
    Ok(())
}
