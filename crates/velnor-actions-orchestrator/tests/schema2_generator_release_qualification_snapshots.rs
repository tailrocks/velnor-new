use super::action_snapshots::{Actions, action};

#[path = "schema2_generator_release_qualification_snapshots/output_bindings.rs"]
mod output_bindings;

pub(super) fn assert_candidate_qualification(
    workflow: &str,
    actions: &Actions,
    job_id: &str,
    action_name: &str,
    build_job: &str,
    directory: &str,
    binary: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let action_body = action(actions, action_name)?;
    let raw_validation = action_body
        .find("Validate exact downloaded candidate files")
        .ok_or("missing raw candidate artifact validation")?;
    let non_executable_raw_file = action_body
        .find(r#"test ! -x \"$path\""#)
        .ok_or("raw candidate file must not arrive executable")?;
    let provenance = action_body
        .find("Verify candidate provenance record")
        .ok_or("missing candidate provenance verification")?;
    let checksum = action_body
        .find("Verify downloaded checksum sidecar")
        .ok_or("missing downloaded checksum verification")?;
    let qualification = action_body
        .find("Qualify downloaded candidate")
        .ok_or("missing exact candidate qualification")?;
    let executable_mode = action_body
        .find("chmod 0500 ")
        .ok_or("candidate binary must be made non-writable before qualification")?;
    let job = super::super::job_body(workflow, job_id)?;
    assert!(
        raw_validation < non_executable_raw_file
            && non_executable_raw_file < provenance
            && provenance < checksum
            && checksum < qualification
            && qualification < executable_mode,
        "{job}"
    );
    assert_candidate_executes_only_after_mode_change(
        action_body,
        executable_mode,
        directory,
        binary,
    )?;
    assert_qualification_action_contract(action_body, directory, binary);
    assert_qualification_job_inputs(workflow, job_id, build_job)?;
    assert_release_fixture_generation_requirements()?;
    Ok(())
}

fn assert_qualification_job_inputs(
    workflow: &str,
    job_id: &str,
    build_job: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::super::job_body(workflow, job_id)?;
    let caller_artifact_id =
        format!("artifact_id: ${{{{ needs.{build_job}.outputs.artifact_id }}}}");
    assert!(job.contains(&caller_artifact_id), "{job}");
    assert!(
        job.contains("manifest_sha256: ${{ needs.candidate-manifest.outputs.manifest_sha256 }}"),
        "{job}"
    );
    Ok(())
}

fn assert_qualification_action_contract(action_body: &str, directory: &str, binary: &str) {
    assert!(
        action_body.contains("artifact-ids: ${{ inputs.artifact_id }}"),
        "{action_body}"
    );
    assert!(
        action_body.contains("inputs:\n  artifact_id:")
            && action_body.contains("  manifest_artifact_id:")
            && action_body.contains("  manifest_sha256:")
            && action_body
                .contains("VELNOR_RELEASE_MANIFEST_SHA256: ${{ inputs.manifest_sha256 }}"),
        "{action_body}"
    );
    assert!(action_body.contains("required: true"), "{action_body}");
    assert!(!action_body.contains("${{ needs."), "{action_body}");
    assert!(
        !action_body.contains("Attest built artifacts"),
        "{action_body}"
    );
    assert!(
        !action_body.contains("actions/upload-artifact@"),
        "{action_body}"
    );
    assert_eq!(
        action_body.matches("Qualify downloaded candidate").count(),
        1
    );
    assert!(action_body.contains("toolchain"), "{action_body}");
    assert!(action_body.contains("--version"), "{action_body}");
    assert!(
        action_body.contains(&format!(
            "scripts/capture-opentofu-goldens.sh check-release \\\"$GITHUB_WORKSPACE/{directory}/{binary}\\\" \\\"$GITHUB_WORKSPACE/manifest-assets/release-manifest.json\\\" \\\"$VELNOR_RELEASE_MANIFEST_SHA256\\\""
        )),
        "{action_body}"
    );
}

fn assert_candidate_executes_only_after_mode_change(
    action_body: &str,
    executable_mode: usize,
    directory: &str,
    binary: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let invocation = action_body
        .find(&format!("./{directory}/{binary} --version"))
        .ok_or("missing candidate version invocation")?;
    assert!(
        executable_mode < invocation,
        "candidate must be made executable only after raw transport validation"
    );
    Ok(())
}

fn assert_release_fixture_generation_requirements() -> Result<(), Box<dyn std::error::Error>> {
    let fixture_check = include_str!("../../../scripts/capture-opentofu-goldens.sh");
    assert!(
        fixture_check.contains("generate --output-dir"),
        "{fixture_check}"
    );
    assert!(fixture_check.contains("hostile-config"), "{fixture_check}");
    assert!(
        fixture_check.contains("unknown_config_field"),
        "{fixture_check}"
    );
    assert!(
        fixture_check.contains(r#"capture_release_case "$case" "$repo""#),
        "{fixture_check}"
    );
    assert!(
        fixture_check.contains(r#"stage_candidate_manifest "$repo" "$case""#),
        "{fixture_check}"
    );
    assert_schema_only_consumer_manifest_input(fixture_check);
    assert!(
        fixture_check.contains("capture_release_dogfood"),
        "{fixture_check}"
    );
    assert!(
        !fixture_check.contains("write_release_fixture_manifest"),
        "{fixture_check}"
    );
    let qualification_helpers =
        include_str!("../../../scripts/generator-release/qualification-goldens.sh");
    assert_candidate_output_binding_validation(qualification_helpers)?;
    assert!(
        qualification_helpers.contains(r#"stage_candidate_manifest "$repo" dogfood"#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers
            .contains(r#"cmp -s "$CANDIDATE_MANIFEST" "$repo/.velnor/release-manifest.json""#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("staged_sha\" != \"$CANDIDATE_MANIFEST_SHA256\""),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("GITHUB_SHA"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("host_target"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("candidate manifest digest does not match candidate CLI"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains(r#"commit_placeholder="$(printf '%040d' 0 | tr '0' 'b')""#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains(r#"jq -er '.commit | strings' "$CANDIDATE_MANIFEST""#),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("candidate manifest contains a malformed source commit"),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("could not normalize candidate manifest commit in $file"),
        "{qualification_helpers}"
    );
    assert_dogfood_cargo_warmup(qualification_helpers)?;
    Ok(())
}

fn assert_candidate_output_binding_validation(
    qualification_helpers: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    output_bindings::assert_qualification_pipeline(qualification_helpers)?;
    let release_case = qualification_helpers
        .find("capture_release_case()")
        .ok_or("release candidate case capture must exist")?;
    let capture_window = &qualification_helpers[release_case..];
    let checker = capture_window
        .find("validate_candidate_output_bindings()")
        .ok_or("trusted output-binding checker must be defined")?;
    let normalization = capture_window
        .find("normalize_candidate_digests()")
        .ok_or("candidate output normalization must be defined")?;
    let checker_source = &capture_window[checker..normalization];
    for handoff in [
        "prepare_qualification_guard || return 1",
        "verify_qualification_guard || return 1",
        "\"$QUALIFICATION_GUARD_BIN\" qualification-output-bindings",
        "\"$CANDIDATE_MANIFEST\" \"$preview/.github\"",
    ] {
        assert!(
            checker_source.contains(handoff),
            "trusted output-binding checker is missing {handoff}"
        );
    }
    assert!(
        !checker_source.contains("$BIN")
            && !checker_source.contains("VELNOR_REPO_POLICY_ACTION")
            && !checker_source.contains("VELNOR_INTERNAL_OP"),
        "candidate CLI must not be asked to validate its own output bindings"
    );
    Ok(())
}

fn assert_schema_only_consumer_manifest_input(fixture_check: &str) {
    assert!(
        fixture_check.contains("write_schema_only_consumer_manifest()")
            && fixture_check.contains(r#"version="$("$BIN" --version | awk"#)
            && fixture_check.contains(
                "[[ ! \"$version\" =~ ^[0-9]+\\.[0-9]+\\.[0-9]+$ ]]",
            )
            && fixture_check.contains(
                "https://github.com/tailrocks/velnor-new/releases/download/v$version/velnor-actions-$version-$target",
            )
            && fixture_check.contains(r#"commit="$(printf '%040d' 0 | tr '0' 'b')""#)
            && fixture_check.contains(r#"digest="$(printf '%064d' 0 | tr '0' 'a')""#)
            && fixture_check.contains("write_schema_only_consumer_manifest \"$repo\""),
        "positive fixtures use the current-version schema-only marker contract: {fixture_check}"
    );
    assert!(
        !fixture_check.contains(r#"cp "$ROOT/fixtures/consumer-release-manifest.json""#),
        "positive fixture capture must not relabel the static schema placeholder: {fixture_check}"
    );
}

fn assert_dogfood_cargo_warmup(
    qualification_helpers: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let normalized = qualification_helpers
        .replace("\\\n", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
normalized.contains(
            r#"rust_version="$(awk -v key=rust -f "$repo/scripts/toml-tool-pin.awk" "$repo/.velnor/version-policy.toml")""#
        )
            && normalized.contains(
                r#"MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false CARGO_HOME="$cargo_home" RUSTUP_HOME="$rustup_home" mise --no-config --no-env --no-hooks exec "rust@$rust_version" -- cargo fetch --locked --manifest-path "$manifest";"#
            ),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains(
            r#"for manifest in "$repo/Cargo.toml" "$repo/crates/velnor-runner/Cargo.toml""#
        ),
        "{qualification_helpers}"
    );
    assert!(
        qualification_helpers.contains("could not fetch dogfood cargo sources"),
        "{qualification_helpers}"
    );
    let dogfood = qualification_helpers
        .find("capture_release_dogfood()")
        .ok_or("dogfood capture must exist")?;
    let window = &qualification_helpers[dogfood..];
    let fetch = window
        .find("cargo fetch --locked")
        .ok_or("dogfood must warm the cargo cache")?;
    let generate = window
        .find("release candidate dogfood generate failed")
        .ok_or("dogfood must run the candidate generate")?;
    assert!(
        fetch < generate,
        "dogfood cargo fetch must precede the candidate generate"
    );
    assert!(
        qualification_helpers.contains("git -C \"$ROOT\" write-tree")
            && qualification_helpers.contains("git -C \"$ROOT\" diff --quiet --")
            && !window_contains_head_archive(qualification_helpers),
        "dogfood must archive the clean indexed candidate tree"
    );
    Ok(())
}

fn window_contains_head_archive(helpers: &str) -> bool {
    helpers.contains("git -C \"$ROOT\" archive --format=tar HEAD")
}

#[test]
fn dogfood_candidate_tree_uses_staged_source_and_rejects_unstaged_edits()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::impl_common::git_fixture;
    use std::process::Command;

    let repository = tempfile::tempdir()?;
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/qualification-goldens.sh");
    let run_git = |args: &[&str]| -> Result<(), Box<dyn std::error::Error>> {
        let output = git_fixture::command(repository.path())?
            .args(args)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()?;
        if !output.status.success() {
            return Err(format!("git {args:?} failed").into());
        }
        Ok(())
    };
    run_git(&["init", "-q"])?;
    std::fs::write(repository.path().join("policy.toml"), "head\n")?;
    run_git(&["add", "policy.toml"])?;
    run_git(&["commit", "-qm", "baseline"])?;
    std::fs::write(repository.path().join("policy.toml"), "staged candidate\n")?;
    run_git(&["add", "policy.toml"])?;

    let capture_tree = || {
        Command::new("bash")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .arg("-c")
            .arg("source \"$1\"; ROOT=\"$2\"; candidate_tree_oid")
            .arg("dogfood-tree-test")
            .arg(&script)
            .arg(repository.path())
            .output()
    };
    let captured = capture_tree()?;
    assert!(captured.status.success());
    let tree = String::from_utf8(captured.stdout)?.trim().to_owned();
    assert_eq!(tree.len(), 40);
    let staged = git_fixture::command(repository.path())?
        .args(["show", &format!("{tree}:policy.toml")])
        .output()?;
    assert!(staged.status.success());
    assert_eq!(staged.stdout, b"staged candidate\n");

    std::fs::write(repository.path().join("policy.toml"), "unstaged attack\n")?;
    let rejected = capture_tree()?;
    assert!(!rejected.status.success());
    assert!(String::from_utf8(rejected.stderr)?.contains("unstaged tracked changes"));
    Ok(())
}
