//! Candidate workflow job assembly over the shared release-build contracts.

use super::super::super::features::{base, finish};
use super::super::{qualification, release_steps, scripts};
use super::{
    AssetNames, CANDIDATE_ASSET_DIR, CANDIDATE_MANIFEST_DIR, CandidateTarget, build_permissions,
    candidate_context_step, download_build_artifact, manifest_artifact_name,
    manifest_digest_script, manifest_script, mise_step, needs_success, read_permissions,
    run_context_outputs, with_if, with_needs, with_permissions,
};
use crate::yaml::Yaml;
use velnor_actions_contract::RELEASE_MANIFEST_FILENAME;

pub(super) fn build_job(target: &CandidateTarget, version: &str) -> (String, Yaml) {
    let mut steps = vec![
        release_steps::checkout_candidate_step(),
        mise_step(),
        candidate_context_step(),
    ];
    steps.push(release_steps::bash_run_step(
        "Install catalog tools and verify MBX",
        &scripts::install_tools(target.os),
    ));
    if target.os == "macos-x64" {
        steps.push(release_steps::bash_run_step(
            "Install pinned Intel Rust target",
            &scripts::install_rust_target(target.target),
        ));
    }
    steps.push(release_steps::bash_run_step(
        "Build exact pull-request source",
        &scripts::build(
            target.target,
            target.os == "macos-x64",
            if target.os == "linux" {
                "linux"
            } else {
                "macos"
            },
            &target.asset,
            &target.sidecar,
            if target.os == "linux" {
                "sha256sum"
            } else {
                "shasum -a 256"
            },
        ),
    ));
    steps.push(release_steps::bash_run_step(
        "Record source-bound candidate provenance",
        &scripts::candidate_provenance(
            version,
            target.target,
            &target.asset,
            &target.sidecar,
            &target.provenance,
            if target.os == "linux" {
                "sha256sum"
            } else {
                "shasum -a 256"
            },
        ),
    ));
    steps.push(release_steps::upload_step_with_id(
        "candidate-assets",
        "Upload pull-request candidate assets",
        &target.artifact,
        &[&target.asset, &target.sidecar, &target.provenance],
    ));
    let needs = ["candidate-gate"];
    let mut fields = with_needs(base(target.name, target.build_runner.clone(), 120), &needs);
    fields = with_if(fields, &needs_success(&needs));
    let mut outputs = run_context_outputs();
    outputs.push((
        "asset_id".to_owned(),
        Yaml::str("${{ steps.candidate-assets.outputs.artifact-id }}"),
    ));
    fields.push(("outputs".to_owned(), Yaml::Map(outputs)));
    finish(
        target.id,
        with_permissions(fields, build_permissions()),
        steps,
    )
}

pub(super) fn manifest_job(hosted: Yaml, version: &str) -> (String, Yaml) {
    let needs = [
        "candidate-gate",
        "candidate-build-linux-x64",
        "candidate-build-macos-arm64",
        "candidate-build-macos-x64",
    ];
    let mut fields = with_needs(
        base("Assemble pull-request candidate manifest", hosted, 30),
        &needs,
    );
    fields.insert(1, ("if".to_owned(), Yaml::str(needs_success(&needs))));
    let mut outputs = run_context_outputs();
    outputs.extend([
        (
            "manifest_id".to_owned(),
            Yaml::str("${{ steps.candidate-manifest.outputs.artifact-id }}"),
        ),
        (
            "manifest_sha256".to_owned(),
            Yaml::str("${{ steps.manifest-digest.outputs.manifest_sha256 }}"),
        ),
    ]);
    fields.push(("outputs".to_owned(), Yaml::Map(outputs)));
    let steps = vec![
        release_steps::checkout_candidate_step(),
        mise_step(),
        candidate_context_step(),
        download_build_artifact(
            "Download Linux candidate assets by ID",
            "candidate-build-linux-x64",
            "linux-assets",
        ),
        download_build_artifact(
            "Download macOS arm64 candidate assets by ID",
            "candidate-build-macos-arm64",
            "macos-assets",
        ),
        download_build_artifact(
            "Download macOS x86_64 candidate assets by ID",
            "candidate-build-macos-x64",
            "macos-intel-assets",
        ),
        release_steps::bash_run_step(
            "Create canonical same-run candidate manifest",
            &manifest_script(version),
        ),
        release_steps::bash_run_step_with_id(
            "manifest-digest",
            "Record immutable candidate manifest digest",
            &manifest_digest_script(),
        ),
        release_steps::upload_step_with_id(
            "candidate-manifest",
            "Upload pull-request candidate manifest",
            &manifest_artifact_name(),
            &[&format!(
                "{CANDIDATE_MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}"
            )],
        ),
    ];
    fields = with_permissions(fields, read_permissions());
    finish("candidate-prepare-manifest", fields, steps)
}

pub(super) fn qualify_job(target: &CandidateTarget, version: &str) -> (String, Yaml) {
    let job_id = target.id.replace("candidate-build", "candidate-qualify");
    let build_id = target.build_id;
    let needs = ["candidate-gate", build_id, "candidate-prepare-manifest"];
    let name = format!("Qualify pull-request candidate / {}", target.target);
    let mut fields = with_needs(base(&name, target.qualify_runner.clone(), 120), &needs);
    fields.insert(1, ("if".to_owned(), Yaml::str(needs_success(&needs))));
    fields = with_permissions(fields, read_permissions());
    fields.push(("outputs".to_owned(), Yaml::Map(run_context_outputs())));
    let manifest_sha = "${{ needs.candidate-prepare-manifest.outputs.manifest_sha256 }}";
    let steps = vec![
        release_steps::checkout_candidate_step(),
        mise_step(),
        candidate_context_step(),
        release_steps::bash_run_step(
            "Install catalog-pinned qualification tools",
            &scripts::install_qualification_toolchain(),
        ),
        release_steps::download_artifact_id_step(
            "Download exact pull-request candidate artifact",
            &format!("${{{{ needs.{build_id}.outputs.asset_id }}}}"),
            CANDIDATE_ASSET_DIR,
        ),
        release_steps::download_artifact_id_step(
            "Download exact same-run candidate manifest",
            "${{ needs.candidate-prepare-manifest.outputs.manifest_id }}",
            CANDIDATE_MANIFEST_DIR,
        ),
        release_steps::bash_run_step_with_env(
            "Bind candidate bytes to same-run manifest",
            &qualification::verify_candidate_script(
                target.target,
                &target.asset,
                &target.sidecar,
                &target.provenance,
                version,
                target.os,
                &AssetNames::for_version(version),
            ),
            "VELNOR_RELEASE_MANIFEST_SHA256",
            manifest_sha,
        ),
        release_steps::bash_run_step_with_env(
            "Run candidate generation and fixture qualification",
            &qualification::qualify_candidate_script(target.target, &target.asset, target.os),
            "VELNOR_RELEASE_MANIFEST_SHA256",
            manifest_sha,
        ),
    ];
    finish(&job_id, fields, steps)
}

pub(super) fn required_job(hosted: Yaml, target_ids: &[&str]) -> (String, Yaml) {
    let mut needs = vec![
        "candidate-gate".to_owned(),
        "candidate-prepare-manifest".to_owned(),
    ];
    needs.extend(target_ids.iter().map(|id| (*id).to_owned()));
    needs.extend(
        target_ids
            .iter()
            .map(|id| id.replace("candidate-build", "candidate-qualify")),
    );
    let need_refs = needs.iter().map(String::as_str).collect::<Vec<_>>();
    let mut fields = with_needs(
        base("Qualify pull-request candidate / all targets", hosted, 10),
        &need_refs,
    );
    fields = with_if(fields, "always()");
    fields = with_permissions(fields, Yaml::Map(Vec::new()));
    finish(
        "candidate-qualification-required",
        fields,
        vec![required_step(&need_refs)],
    )
}

fn required_step(needs: &[&str]) -> Yaml {
    let expected = [
        ("VELNOR_EXPECT_RUN_ID", "${{ github.run_id }}"),
        ("VELNOR_EXPECT_RUN_ATTEMPT", "${{ github.run_attempt }}"),
        ("VELNOR_EXPECT_SOURCE_SHA", "${{ github.sha }}"),
    ];
    let mut env = expected
        .iter()
        .map(|(name, value)| ((*name).to_owned(), Yaml::str(*value)))
        .collect::<Vec<_>>();
    let mut script = String::from("set -eu\n");
    for job in needs {
        let key = job.replace('-', "_").to_ascii_uppercase();
        for (suffix, output, expected) in [
            ("RESULT", "result", None),
            ("RUN_ID", "outputs.run_id", Some("VELNOR_EXPECT_RUN_ID")),
            (
                "RUN_ATTEMPT",
                "outputs.run_attempt",
                Some("VELNOR_EXPECT_RUN_ATTEMPT"),
            ),
            (
                "SOURCE_SHA",
                "outputs.source_sha",
                Some("VELNOR_EXPECT_SOURCE_SHA"),
            ),
        ] {
            let name = format!("VELNOR_NEEDS_{key}_{suffix}");
            env.push((
                name.clone(),
                Yaml::str(format!("${{{{ needs.{job}.{output} }}}}")),
            ));
            script.push_str("test \"$");
            script.push_str(&name);
            script.push_str("\" = \"");
            if let Some(expected) = expected {
                script.push('$');
                script.push_str(expected);
            } else {
                script.push_str("success");
            }
            script.push_str("\"\n");
        }
    }
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Require current-run candidate tasks"),
        ),
        ("env".to_owned(), Yaml::Map(env)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(script)),
    ])
}

pub(super) fn gate_job(hosted: Yaml) -> (String, Yaml) {
    let mut fields = with_permissions(
        with_if(
            base("Pull-request candidate source gate", hosted, 10),
            "github.event_name == 'pull_request' && github.repository == 'tailrocks/velnor-new' && github.event.pull_request.base.ref == 'main'",
        ),
        build_permissions(),
    );
    fields.push(("outputs".to_owned(), Yaml::Map(run_context_outputs())));
    finish(
        "candidate-gate",
        fields,
        vec![
            release_steps::checkout_candidate_step(),
            candidate_context_step(),
        ],
    )
}
