// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

fn fixed_script(section: &str) -> Result<String, Box<dyn Error>> {
    let mut bytes = Vec::new();
    for line in section
        .lines()
        .filter(|line| line.contains("printf '%s' '"))
    {
        let encoded = line
            .split("printf '%s' '")
            .nth(1)
            .and_then(|tail| tail.split('\'').next())
            .ok_or("fixed script chunk")?;
        bytes.extend(STANDARD.decode(encoded)?);
    }
    Ok(String::from_utf8(bytes)?)
}

fn assert_publish_has_no_checkout(
    workflow: &str,
    build: &str,
    publish: &str,
    next: Option<&str>,
) -> Result<(), Box<dyn Error>> {
    let publish_section = job_section(workflow, &format!("{publish}:"), next)?;
    assert!(publish_section.contains("contents: write"));
    assert!(
        !publish_section.contains(CHECKOUT_USES) && !publish_section.contains("name: Check out"),
        "{publish} checks out the repository with contents write"
    );
    let script = fixed_script(publish_section)?;
    assert!(script.contains("release_eligibility"));
    assert!(
        !script.contains("git rev-parse"),
        "{publish} publication script still reads a checkout"
    );
    let build_section = job_section(workflow, &format!("{build}:"), Some("attest-"))?;
    assert!(build_section.contains("contents: read"));
    assert!(build_section.contains(CHECKOUT_USES));
    Ok(())
}

#[test]
fn release_family_publish_jobs_hold_contents_write_without_a_checkout() -> Result<(), Box<dyn Error>>
{
    let files = render_schema2_workflows(&request()?)?;
    let images = find_file(&files, ".github/workflows/product-release-images.yml")?;
    assert_publish_has_no_checkout(images, "build-images", "publish-images", None)?;

    let binary_request = Schema2WorkflowRequest {
        runner_image_family: None,
        workflows: BTreeSet::from([RoutingWorkflow::MacosBinaryRelease]),
        ..request()?
    };
    let binary_files = render_schema2_workflows(&binary_request)?;
    let binary = find_file(
        &binary_files,
        ".github/workflows/product-release-binary.yml",
    )?;
    let publish = job_section(binary, "publish-binary:", None)?;
    assert!(publish.contains("contents: write"));
    assert!(!publish.contains(CHECKOUT_USES));
    assert!(!fixed_script(publish)?.contains("git rev-parse"));
    Ok(())
}
