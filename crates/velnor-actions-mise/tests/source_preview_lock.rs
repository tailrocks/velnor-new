// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use velnor_actions_contract::{PlanGenerator, PlanPreviewChannel, PlanPreviewIdentity};
use velnor_actions_mise::catalog::lock::parse_generator_lock;

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CANDIDATE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const RECEIPT_SHA: &str = "2d133dc7766edd85d88b9ec8d132f075ebad232849500277ef3e6f5f93beefce";
const RECEIPT: &str = r#"{"ci":{"run_attempt":1,"run_id":12,"workflow_path":".github/workflows/ci.yml","workflow_ref":"tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main","workflow_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"payload":{"name":"velnor-actions-x86_64-unknown-linux-gnu","sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","size":12345,"target":"x86_64-unknown-linux-gnu"},"qualification":{"run_attempt":1,"run_id":13,"workflow_path":".github/workflows/source-qualification.yml","workflow_ref":"tailrocks/velnor-new/.github/workflows/source-qualification.yml@refs/heads/main","workflow_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"release_id":10,"repository":"tailrocks/velnor-new","schema":1,"signer":{"run_attempt":1,"run_id":11,"workflow_path":".github/workflows/product-release-generator.yml","workflow_ref":"tailrocks/velnor-new/.github/workflows/product-release-generator.yml@refs/tags/sdk-preview-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","workflow_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"source_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","tag":"sdk-preview-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","version":"0.1.75"}"#;

#[test]
fn stable_plan_generator_retains_its_existing_wire_shape() -> Result<(), Box<dyn std::error::Error>>
{
    let mut generator = PlanGenerator {
        version: "0.1.74".to_owned(),
        target: "x86_64-unknown-linux-gnu".to_owned(),
        sha256: "a".repeat(64),
        preview: None,
    };
    let old = format!(
        "{{\"version\":\"0.1.74\",\"target\":\"x86_64-unknown-linux-gnu\",\"sha256\":\"{}\"}}",
        "a".repeat(64)
    );
    assert_eq!(serde_json::to_string(&generator)?, old);
    generator.preview = Some(PlanPreviewIdentity {
        channel: PlanPreviewChannel::SourcePreview,
        source_sha: SOURCE.to_owned(),
        receipt_sha256: "b".repeat(64),
    });
    let encoded = serde_json::to_value(generator)?;
    assert_eq!(encoded["preview"]["channel"], "source-preview");
    assert_eq!(encoded["preview"]["source_sha"], SOURCE);
    Ok(())
}

fn lock_text() -> Result<String, std::fmt::Error> {
    let mut text = include_str!("../../../.velnor/generator.lock").to_owned();
    write!(
        &mut text,
        "\n[planner-preview]\nschema = 1\nrepository = \"tailrocks/velnor-new\"\nversion = \"0.1.75\"\nsource_sha = \"{SOURCE}\"\ntag = \"sdk-preview-{SOURCE}\"\nrelease_id = 10\n"
    )?;
    for (role, path, id) in [
        (
            "signer",
            ".github/workflows/product-release-generator.yml",
            11,
        ),
        ("ci", ".github/workflows/ci.yml", 12),
        (
            "qualification",
            ".github/workflows/source-qualification.yml",
            13,
        ),
    ] {
        let git_ref = if role == "signer" {
            format!("refs/tags/sdk-preview-{SOURCE}")
        } else {
            "refs/heads/main".to_owned()
        };
        write!(
            &mut text,
            "{role}_workflow_path = \"{path}\"\n{role}_workflow_ref = \"tailrocks/velnor-new/{path}@{git_ref}\"\n{role}_workflow_sha = \"{SOURCE}\"\n{role}_run_id = {id}\n{role}_run_attempt = 1\n"
        )?;
    }
    write!(
        &mut text,
        "payload_target = \"x86_64-unknown-linux-gnu\"\npayload_name = \"velnor-actions-x86_64-unknown-linux-gnu\"\npayload_size = 12345\npayload_sha256 = \"{}\"\nreceipt_sha256 = \"{RECEIPT_SHA}\"\nbundle_sha256 = \"{}\"\n",
        "c".repeat(64),
        "d".repeat(64)
    )?;
    Ok(text)
}

#[test]
fn preview_lock_binds_exact_receipt_without_changing_stable_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let original = parse_generator_lock(include_str!("../../../.velnor/generator.lock"))?;
    assert!(original.source_preview.is_none());
    assert!(original.staged_source_preview.is_none());
    let parsed = parse_generator_lock(&lock_text()?)?;
    assert_eq!(parsed.generator.commit, original.generator.commit);
    assert_eq!(
        parsed.generator.binaries.len(),
        original.generator.binaries.len()
    );
    let preview = parsed.source_preview.ok_or("preview missing")?;
    let staged_text = lock_text()?.replace("[planner-preview]", "[planner-preview-staged]");
    let staged = parse_generator_lock(&staged_text)?;
    assert!(staged.source_preview.is_none());
    assert_eq!(staged.staged_source_preview.as_ref(), Some(&preview));
    preview.validate_receipt_bytes(RECEIPT.as_bytes(), CANDIDATE)?;
    let mut wrong_digest = preview.clone();
    wrong_digest.receipt_sha256 = "e".repeat(64);
    assert!(
        wrong_digest
            .validate_receipt_bytes(RECEIPT.as_bytes(), CANDIDATE)
            .is_err()
    );
    assert!(
        preview
            .validate_receipt_bytes(RECEIPT.as_bytes(), SOURCE)
            .is_err()
    );
    assert!(
        preview
            .validate_receipt_bytes(format!("{RECEIPT} ").as_bytes(), CANDIDATE)
            .is_err()
    );
    assert!(
        preview
            .validate_receipt_bytes(&vec![b'x'; 65_537], CANDIDATE)
            .is_err()
    );
    Ok(())
}

#[test]
fn preview_lock_rejects_inconsistent_or_ambiguous_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let good = lock_text()?;
    for (from, to) in [
        ("release_id = 10", "release_id = 0"),
        ("ci_run_attempt = 1", "ci_run_attempt = 0"),
        (
            "payload_target = \"x86_64-unknown-linux-gnu\"",
            "payload_target = \"aarch64-apple-darwin\"",
        ),
        ("refs/heads/main", "refs/heads/other"),
        ("refs/tags/sdk-preview-", "refs/heads/sdk-preview-"),
        ("sdk-preview-", "preview-"),
        ("tag = \"sdk-preview-", "tag = \"other-"),
        ("receipt_sha256 = ", "missing_sha256 = "),
        ("schema = 1\nrepository = ", "schema = 2\nrepository = "),
    ] {
        assert!(
            parse_generator_lock(&good.replacen(from, to, 1)).is_err(),
            "accepted {from}"
        );
    }
    assert!(parse_generator_lock(&format!("{good}\n[planner-preview]\nschema = 1\n")).is_err());
    assert!(parse_generator_lock(&format!("{good}\nunknown = \"x\"\n")).is_err());
    Ok(())
}
