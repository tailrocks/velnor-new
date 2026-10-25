use super::*;

#[test]
fn published_callee_is_generic_and_uses_existing_seed_runtime_operations() {
    let pins = crate::schema2::product_release_test_pins::test_pins();
    let rendered = render_homebrew_adoption_callee("0.1.53", &pins).expect("render callee");
    let path = rendered.file.path.clone();
    let bytes = rendered.file.bytes;

    assert_eq!(path, PATH);
    for input in [
        "transition_id",
        "sdk_version",
        "sdk_source_sha",
        "sdk_manifest_sha256",
    ] {
        assert!(
            bytes.contains(&format!("{input}:")),
            "missing input {input}"
        );
        assert!(bytes.contains("required: true"));
    }
    for operation in [
        "VELNOR_INTERNAL_OP: write-request-v1",
        "plan-v1-request.json",
        "plan-v1",
        "execute-native-homebrew-check-v1",
        "fetch-reports-v1",
        "merge-v1-request.json",
        "merge-v1",
    ] {
        assert!(
            bytes.contains(operation),
            "missing standard operation {operation}"
        );
    }
    assert!(bytes.contains("tailrocks/velnor-new/.github/workflows/product-release-generator.yml"));
    assert!(bytes.contains("--source-ref refs/heads/main"));
    assert!(bytes.contains("--source-digest"));
    assert!(bytes.contains("--signer-digest"));
    assert!(bytes.contains("VELNOR_HOMEBREW_ADOPTION_SOURCE_SHA"));
    assert!(!bytes.contains("VELNOR_HOME_BREW_ADOPTION_"));
    assert!(
        bytes.contains("fromJSON(needs.plan.outputs.matrix)"),
        "missing dynamic matrix expression: {bytes}"
    );
    assert!(
        !bytes.contains("include:"),
        "nested matrix include: {bytes}"
    );
    assert!(bytes.contains("${{ matrix.artifact_id }}"));
    assert!(bytes.contains(
        "${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}"
    ));
    assert!(bytes.contains("fetch-depth: 0"));
    assert!(bytes.contains("submodules: false"));
    assert!(bytes.contains("plan_attempt"));
    assert!(bytes.contains("needs.plan.outputs.plan_attempt"));
    assert!(bytes.contains("required-gate:"));
    assert!(bytes.contains("needs.required.result != 'success'"));
    assert!(bytes.contains(crate::final_steps::FINAL_ARTIFACT_NAME));
    assert!(bytes.contains(crate::final_steps::FINAL_ARTIFACT_PATH));
    assert!(
        bytes.find("Merge reports").expect("merge step")
            < bytes
                .find("Publish final report")
                .expect("final report step")
    );
    assert!(bytes.contains("contents: read"));
    assert!(bytes.contains("actions: read"));
    assert!(bytes.contains("attestations: read"));
    assert!(bytes.contains("${{ inputs.sdk_source_sha }}"));
    assert!(bytes.contains("${{ inputs.sdk_manifest_sha256 }}"));
}

#[test]
fn callee_required_check_uses_the_authoritative_cancellation_gate_factory() {
    let pins = crate::schema2::product_release_test_pins::test_pins();
    let document = workflow_document(&pins).expect("render workflow document");
    let Yaml::Map(root) = document else {
        panic!("workflow mapping");
    };
    let jobs = root
        .iter()
        .find_map(|(key, value)| (key == "jobs").then_some(value))
        .expect("jobs field");
    let Yaml::Map(jobs) = jobs else {
        panic!("jobs mapping");
    };
    let actual = jobs
        .iter()
        .find_map(|(id, job)| (id == velnor_actions_contract::REQUIRED_GATE_JOB_ID).then_some(job))
        .expect("Required gate job");
    let expected = crate::required_gate::required_gate_job_yaml(RUNNER)
        .expect("factory gate")
        .1;
    assert_eq!(actual, &expected);

    let Yaml::Map(fields) = actual else {
        panic!("gate mapping");
    };
    assert!(fields.iter().any(|(key, value)| {
        key == "if" && value == &Yaml::str(velnor_actions_contract::REQUIRED_GATE_CONDITION)
    }));
    let steps = fields
        .iter()
        .find_map(|(key, value)| (key == "steps").then_some(value))
        .expect("gate steps");
    let Yaml::Seq(steps) = steps else {
        panic!("gate step sequence");
    };
    let Yaml::Map(step) = &steps[0] else {
        panic!("gate shell step");
    };
    assert!(step.iter().any(|(key, value)| {
        key == "if" && value == &Yaml::str(velnor_actions_contract::REQUIRED_GATE_FAIL_CONDITION)
    }));
}

#[cfg(unix)]
#[test]
fn bootstrap_smoke_authenticates_before_executing_the_staged_binary() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn executable(path: &std::path::Path, body: &str) {
        fs::write(path, body).expect("write helper");
        let mut permissions = fs::metadata(path).expect("helper metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("helper executable");
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "velnor-adoption-smoke-{}-{nonce}",
        std::process::id()
    ));
    let bin = root.join("bin");
    fs::create_dir_all(&bin).expect("create temp bin");
    let log = root.join("calls.log");
    let gh = bin.join("gh");
    executable(
        &gh,
        &format!(
            "#!/bin/bash\nprintf '%s\\n' \"$*\" >> '{}'\nif [[ \"$1 $2\" == 'release download' ]]; then\n  shift 2; tag=; dir=; pattern=\n  while (($#)); do case \"$1\" in v*) tag=$1;; --dir) dir=$2; shift;; --pattern) pattern=$2; shift;; esac; shift; done\n  if [[ $pattern == release-manifest.json ]]; then printf '{{}}' > \"$dir/$pattern\"; else printf '#!/bin/bash\\necho staged-version\\n' > \"$dir/$pattern\"; chmod +x \"$dir/$pattern\"; fi\nfi\n",
            log.display()
        ),
    );
    let jq = bin.join("jq");
    executable(
        &jq,
        &format!(
            "#!/bin/bash\nprintf 'jq %s\\n' \"$*\" >> '{}'\nif [[ $1 == -er ]]; then printf '%064d\\n' 0; fi\n",
            log.display()
        ),
    );
    let checksum = bin.join("sha256sum");
    executable(
        &checksum,
        &format!(
            "#!/bin/bash\nprintf 'sha256sum %s\\n' \"$*\" >> '{}'\ncat >/dev/null\n",
            log.display()
        ),
    );

    let bootstrap = super::bootstrap::bootstrap_script();
    assert!(bootstrap.contains("asset_sha=\"$(/usr/bin/jq"));
    assert!(bootstrap.contains("readonly asset_sha\n"));
    assert!(!bootstrap.contains("readonly asset_sha=\"$("));
    let mut script = bootstrap.replace("/usr/bin/jq", &jq.display().to_string());
    script.push('\n');
    let status = Command::new("/bin/bash")
        .arg("-c")
        .arg(script)
        .env("RUNNER_TEMP", root.join("runner"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("VELNOR_ADOPTION_GH", gh.display().to_string())
        .env("GH_TOKEN", "test-token")
        .env("VELNOR_HOMEBREW_ADOPTION_VERSION", "0.1.53")
        .env("VELNOR_HOMEBREW_ADOPTION_SOURCE_SHA", "c".repeat(40))
        .env("VELNOR_HOMEBREW_ADOPTION_MANIFEST_SHA256", "d".repeat(64))
        .status()
        .expect("run bootstrap smoke");
    assert!(status.success(), "bootstrap smoke status: {status}");
    let calls = fs::read_to_string(&log).expect("read fake tool calls");
    assert!(calls.contains(
        "release download v0.1.53 --repo tailrocks/velnor-new --pattern release-manifest.json"
    ));
    assert!(calls.contains("attestation verify"));
    assert!(calls.contains("product-release-generator.yml --signer-digest"));
    assert!(calls.contains("--source-ref refs/heads/main --source-digest"));
    assert!(calls.contains("velnor-actions-0.1.53-x86_64-unknown-linux-gnu"));
    assert!(calls.contains("sha256sum --check --status"));
    fs::remove_dir_all(root).expect("remove smoke temp");
}
