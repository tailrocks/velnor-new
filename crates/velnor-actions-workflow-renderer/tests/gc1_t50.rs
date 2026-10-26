// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::io;

use velnor_actions_actionlint::actions::MISE_ACTION_SHA;
use velnor_actions_contract::RoutingWorkflow;
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::workflow::cache_family::GC1_MAX_GENERATION_SLOTS;
use velnor_actions_contract::workflow::qualification_cache_lineage::{
    QUALIFICATION_CACHE_MAX_GENERATIONS, validate_qualification_cache_generation,
};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};
use velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude;
use velnor_actions_workflow_renderer::{
    MbxQualificationPins, MisePinQualificationPins, RenderedFile, Schema2WorkflowRequest,
    render_schema2_workflows,
};

const MBX_VERSION: &str = "1.91.0";
const RUST_VERSION: &str = "1.91.0";
const BOUNDED_GENERATION: &str = "velnor-mbx-1.91.0-share-out-dir-disabled-v1";
const QUALIFICATION_WORKFLOW: &str = ".github/workflows/qualification.yml";
const WRITER_JOB: &str = "mbx-cache-write-hosted";
const READER_JOB: &str = "mbx-cache-read-hosted";
const AFTER_MBX_JOB: &str = "mise-pin-linux-x64:";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn forbid(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Err(fail(&format!("{label} kept:{needle}")))
    } else {
        Ok(())
    }
}

fn require_order(
    haystack: &str,
    first: &str,
    second: &str,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let first_pos = haystack
        .find(first)
        .ok_or_else(|| fail(&format!("{label} lost:{first}")))?;
    let second_pos = haystack
        .find(second)
        .ok_or_else(|| fail(&format!("{label} lost:{second}")))?;
    if first_pos < second_pos {
        Ok(())
    } else {
        Err(fail(&format!("{label} misordered:{first}:{second}")))
    }
}

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

fn candidate_action_uses() -> String {
    format!("jdx/mr-boxington-action@{}", "a".repeat(40))
}

fn verify_rust_argv() -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!("{} exit 0", credential_unset_prelude()),
    ]
}

fn mbx_pins() -> MbxQualificationPins {
    MbxQualificationPins {
        mise_setup: mise_setup(),
        candidate_action_uses: candidate_action_uses(),
        mbx_version: MBX_VERSION.to_owned(),
        rust_version: RUST_VERSION.to_owned(),
        verify_rust_argv: verify_rust_argv(),
    }
}

fn mise_pin_pins() -> MisePinQualificationPins {
    MisePinQualificationPins {
        linux_x86_64_setup: mise_setup(),
        macos_x86_64_setup: mise_setup(),
    }
}

fn request() -> Result<Schema2WorkflowRequest, Box<dyn Error>> {
    Ok(Schema2WorkflowRequest {
        version: "0.1.55".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::Qualification]),
        mbx_qualification: Some(mbx_pins()),
        mise_pin_qualification: Some(mise_pin_pins()),
        product_release: None,
    })
}

fn request_with(
    mbx_qualification: Option<MbxQualificationPins>,
    mise_pin_qualification: Option<MisePinQualificationPins>,
) -> Result<Schema2WorkflowRequest, Box<dyn Error>> {
    let mut built = request()?;
    built.mbx_qualification = mbx_qualification;
    built.mise_pin_qualification = mise_pin_qualification;
    Ok(built)
}

fn find_file<'a>(files: &'a [RenderedFile], path: &str) -> Result<&'a str, Box<dyn Error>> {
    files
        .iter()
        .find(|file| file.path == path)
        .map(|file| file.bytes.as_str())
        .ok_or_else(|| fail(&format!("render lost its file:{path}")))
}

fn mbx_section(text: &str) -> Result<&str, Box<dyn Error>> {
    let begin = text
        .find(WRITER_JOB)
        .ok_or_else(|| fail("render lost its writer job"))?;
    let tail = &text[begin..];
    let close = tail
        .find(AFTER_MBX_JOB)
        .ok_or_else(|| fail("render lost its job boundary"))?;
    Ok(&tail[..close])
}

fn writer_section(text: &str) -> Result<&str, Box<dyn Error>> {
    let section = mbx_section(text)?;
    let close = section
        .find(READER_JOB)
        .ok_or_else(|| fail("render lost its reader job"))?;
    Ok(&section[..close])
}

fn reader_section(text: &str) -> Result<&str, Box<dyn Error>> {
    let section = mbx_section(text)?;
    let begin = section
        .find(READER_JOB)
        .ok_or_else(|| fail("render lost its reader job"))?;
    Ok(&section[begin..])
}

fn generation_lines(text: &str) -> Result<Vec<&str>, Box<dyn Error>> {
    let lines: Vec<&str> = mbx_section(text)?
        .lines()
        .filter(|line| line.contains("cache-generation:"))
        .collect();
    if lines.len() != 2 {
        return Err(fail(&format!(
            "bounded writer left its generation count:{}",
            lines.len()
        )));
    }
    Ok(lines)
}

fn check_budget_binding() -> Result<(), Box<dyn Error>> {
    if QUALIFICATION_CACHE_MAX_GENERATIONS != GC1_MAX_GENERATION_SLOTS
        || QUALIFICATION_CACHE_MAX_GENERATIONS != 2
    {
        return Err(fail("qualification writer left the shared budget"));
    }
    validate_qualification_cache_generation(&mbx_cache_generation(MBX_VERSION), MBX_VERSION)
        .map_err(|error| fail(&format!("bounded generation rejected:{error}")))?;
    if mbx_cache_generation(MBX_VERSION) != BOUNDED_GENERATION {
        return Err(fail("shared generation drifted from its owned slot"));
    }
    Ok(())
}

fn check_bounded_generations(text: &str) -> Result<(), Box<dyn Error>> {
    for line in generation_lines(text)? {
        require(line, BOUNDED_GENERATION, "bounded writer")?;
        for marker in ["run_id", "run_attempt", "github.sha", "${{"] {
            forbid(line, marker, "bounded writer")?;
        }
    }
    Ok(())
}

fn check_hosted_admission(writer: &str, reader: &str) -> Result<(), Box<dyn Error>> {
    for section in [writer, reader] {
        require(section, "runs-on: ubuntu-26.04", "hosted writer")?;
        require(section, "github.ref_protected == true", "hosted writer")?;
        require(section, "refs/heads/main", "hosted writer")?;
        forbid(section, "self-hosted", "hosted writer")?;
    }
    require_order(writer, WRITER_JOB, "actions: write", "writer policy")?;
    require(reader, "actions: read", "reader policy")?;
    forbid(reader, "actions: write", "reader policy")?;
    forbid(writer, "needs:", "writer policy")?;
    require_order(
        reader,
        READER_JOB,
        "- mbx-cache-write-hosted",
        "reader order",
    )?;
    require(writer, "save-on-workflow-dispatch: \"true\"", "writer save")?;
    require(
        reader,
        "save-on-workflow-dispatch: \"false\"",
        "reader save",
    )?;
    Ok(())
}

fn check_snapshot_observation(section: &str) -> Result<(), Box<dyn Error>> {
    for marker in [
        "github-cache-mode: objects",
        "isolate-objects-cache",
        "MBX_SHARE_OUT_DIR",
        "MBX_GC_AUTO",
        "mbx clean",
        "mbx cache stats",
        "mbx stats",
        ".objects > 0",
        ".savings.cached_compilations > 0",
    ] {
        require(section, marker, "snapshot observation")?;
    }
    for marker in ["actions/download-artifact", "actions/upload-artifact"] {
        forbid(section, marker, "snapshot observation")?;
    }
    Ok(())
}

#[test]
fn gc1_t50_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_budget_binding()?;
    let text = find_file(
        &render_schema2_workflows(&request()?)?,
        QUALIFICATION_WORKFLOW,
    )?
    .to_owned();
    require_order(&text, WRITER_JOB, READER_JOB, "writer topology")?;
    check_bounded_generations(&text)?;
    check_hosted_admission(writer_section(&text)?, reader_section(&text)?)?;
    check_snapshot_observation(mbx_section(&text)?)?;
    Ok(())
}

fn check_rejects_generation(
    generation: &str,
    version: &str,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    if validate_qualification_cache_generation(generation, version).is_ok() {
        return Err(fail(&format!("bounded writer accepted:{label}")));
    }
    Ok(())
}

fn check_render_rejected(
    built: &Schema2WorkflowRequest,
    marker: &str,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    match render_schema2_workflows(built) {
        Ok(_) => Err(fail(&format!("render accepted:{label}"))),
        Err(error) => {
            if format!("{error}").contains(marker) {
                Ok(())
            } else {
                Err(fail(&format!("render rejected without its reason:{label}")))
            }
        }
    }
}

#[test]
fn gc1_t50_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_generation(
        &format!(
            "{BOUNDED_GENERATION}-run-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}-${{{{ github.sha }}}}"
        ),
        MBX_VERSION,
        "run_specific_generation",
    )?;
    check_rejects_generation("", MBX_VERSION, "empty_generation")?;
    check_rejects_generation(BOUNDED_GENERATION, "9.9.9", "foreign_version")?;
    check_rejects_generation(
        "velnor-mbx-1.91.0-share-out-dir-disabled-v2",
        MBX_VERSION,
        "unowned_slot",
    )?;
    check_rejects_generation("${{ github.sha }}", MBX_VERSION, "template_generation")?;
    check_rejects_generation(&"v".repeat(513), MBX_VERSION, "oversized_generation")?;
    let mut bad_uses = mbx_pins();
    bad_uses.candidate_action_uses = format!("actions/checkout@{}", "b".repeat(40));
    check_render_rejected(
        &request_with(Some(bad_uses), Some(mise_pin_pins()))?,
        "not_mbx_action",
        "foreign_action",
    )?;
    let mut bad_version = mbx_pins();
    bad_version.mbx_version = "1.0".to_owned();
    check_render_rejected(
        &request_with(Some(bad_version), Some(mise_pin_pins()))?,
        "bad_mbx_version",
        "loose_version",
    )?;
    check_render_rejected(
        &request_with(None, Some(mise_pin_pins()))?,
        "missing_mbx_qualification_pins",
        "missing_pins",
    )?;
    let mut mismatched = mise_pin_pins();
    mismatched.macos_x86_64_setup.version = "9.9.9".to_owned();
    check_render_rejected(
        &request_with(Some(mbx_pins()), Some(mismatched))?,
        "mise_pin_qualification_version_mismatch",
        "mismatched_pins",
    )?;
    Ok(())
}
