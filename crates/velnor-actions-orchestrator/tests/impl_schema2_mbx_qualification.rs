//! Security assertions for the protected-main MBX cache round trip.

use std::fs;
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_actionlint::actions::{
    MR_BOXINGTON_ACTION_CANDIDATE_SHA, MR_BOXINGTON_ACTION_SHA,
};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

const CACHE_GENERATION: &str = "velnor-qualification-mbx-1.21.1-share-out-dir-disabled-v1-gc-auto-off-final-clean-v1-action-d0825fbaf3cc36ca2609aa38e71046265a1f1e37";

fn assert_candidate_ref(writer: &str) {
    assert!(
        writer.contains(&format!(
            "uses: jdx/mr-boxington-action@{MR_BOXINGTON_ACTION_CANDIDATE_SHA}"
        )),
        "the experiment must invoke its exact immutable candidate"
    );
    assert!(
        !writer.contains(&format!(
            "uses: jdx/mr-boxington-action@{MR_BOXINGTON_ACTION_SHA}"
        )),
        "qualification must not silently substitute the production pin"
    );
}

fn assert_candidate_owner(job: &str) {
    for fragment in [
        "version: 1.21.1",
        "toolchain: 1.98.1",
        "isolate-objects-cache: \"true\"",
        "RUSTUP_TOOLCHAIN: 1.98.1",
        "MBX_SHARE_OUT_DIR: \"0\"",
        "CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo",
    ] {
        assert!(job.contains(fragment), "{job}");
    }
}

#[test]
fn protected_main_mbx_roundtrip_is_run_bound_and_read_only_on_restore() -> TestResult {
    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;
    let writer = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-write-hosted")?;
    let reader = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-read-hosted")?;
    assert_candidate_ref(writer);
    assert_writer(writer);
    assert_reader(reader);
    assert_lifecycle_cleanup(
        writer,
        "Compile MBX cache probe",
        Some("Sample runner disk after cleanup"),
    );
    assert_lifecycle_cleanup(reader, "Require reused compilation", None);
    Ok(())
}

fn assert_writer(writer: &str) {
    assert!(
        writer.contains("inputs.mode == 'mbx-cache-roundtrip' && github.ref == 'refs/heads/main' && github.ref_protected == true"),
        "{writer}"
    );
    assert!(writer.contains("actions: write"), "{writer}");
    assert!(writer.contains("ACTIONS_CACHE_MODE: write"), "{writer}");
    assert!(writer.contains("MBX_GC_AUTO: \"0\""), "{writer}");
    assert!(
        writer.contains("save-on-workflow-dispatch: \"true\""),
        "{writer}"
    );
    assert!(
        writer.contains(&format!("{CACHE_GENERATION}-run-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}-${{{{ github.sha }}}}")),
        "{writer}"
    );
    assert!(writer.contains("version: 1.21.1"), "{writer}");
    assert!(writer.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{writer}");
    assert!(
        writer
            .contains("CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo"),
        "{writer}"
    );
    assert_candidate_owner(writer);
    assert!(writer.contains("rustc --print sysroot"), "{writer}");
    assert!(
        writer.contains("df -B1 -P \\\"$RUNNER_TEMP\\\""),
        "{writer}"
    );
    assert!(writer.contains("df -i -P \\\"$RUNNER_TEMP\\\""), "{writer}");
}

fn assert_reader(reader: &str) {
    assert!(
        reader.contains("inputs.mode == 'mbx-cache-roundtrip' && github.ref == 'refs/heads/main' && github.ref_protected == true"),
        "{reader}"
    );
    assert!(
        reader.contains("needs:\n      - mbx-cache-write-hosted"),
        "{reader}"
    );
    assert!(reader.contains("actions: read"), "{reader}");
    assert!(reader.contains("ACTIONS_CACHE_MODE: read"), "{reader}");
    assert!(reader.contains("MBX_GC_AUTO: \"0\""), "{reader}");
    assert!(
        reader.contains("steps.mbx_cache.outputs.cache-hit"),
        "{reader}"
    );
    assert!(
        reader.contains(&format!("{CACHE_GENERATION}-run-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}-${{{{ github.sha }}}}")),
        "{reader}"
    );
    assert!(
        reader.contains("test \\\"$CACHE_HIT\\\" = 'false'"),
        "{reader}"
    );
    assert!(
        reader.contains("save-on-workflow-dispatch: \"false\""),
        "{reader}"
    );
    assert!(reader.contains("mbx cache stats --json"), "{reader}");
    assert!(reader.contains("set -e -o pipefail; df -B1 -P"), "{reader}");
    assert!(reader.contains(".objects > 0"), "{reader}");
    assert!(
        reader.contains(".savings.cached_compilations > 0"),
        "{reader}"
    );
    assert!(reader.contains("version: 1.21.1"), "{reader}");
    assert!(
        reader.contains("df -B1 -P \\\"$RUNNER_TEMP\\\""),
        "{reader}"
    );
    assert!(reader.contains("df -i -P \\\"$RUNNER_TEMP\\\""), "{reader}");
    assert!(
        reader.contains("tee \\\"$RUNNER_TEMP/mbx-object-stats.json\\\""),
        "{reader}"
    );
    assert!(
        reader.contains("tee \\\"$RUNNER_TEMP/mbx-reuse-stats.json\\\""),
        "{reader}"
    );
    assert!(
        !reader.contains("tee \\\"$RUNNER_TEMP/mbx-object-stats.json\\\" | jq"),
        "{reader}"
    );
    assert!(
        reader.contains("jq -e '.objects > 0' \\\"$RUNNER_TEMP/mbx-object-stats.json\\\""),
        "{reader}"
    );
    assert!(
        reader.contains(
            "jq -e '.savings.cached_compilations > 0' \\\"$RUNNER_TEMP/mbx-reuse-stats.json\\\""
        ),
        "{reader}"
    );
    assert!(reader.contains("RUSTUP_TOOLCHAIN: 1.98.1"), "{reader}");
    assert!(
        reader
            .contains("CARGO_HOME: ${{ github.workspace }}/.velnor-mbx-cache-qualification/cargo"),
        "{reader}"
    );
    assert_candidate_owner(reader);
}

fn assert_lifecycle_cleanup(job: &str, final_consumer: &str, post_clean_sample: Option<&str>) {
    let action = job.find("      - name: Restore MBX objects\n");
    let ready = job.find("      - name: Verify MBX action identity and write policy\n");
    let consumer = job.find(&format!("      - name: {final_consumer}\n"));
    let clean = job.find("      - name: Clean MBX workspace outputs\n");
    assert!(
        action.is_some_and(|position| ready.is_some_and(|ready| position < ready))
            && ready.is_some_and(|ready| consumer.is_some_and(|consumer| ready < consumer))
            && consumer.is_some_and(|consumer| clean.is_some_and(|clean| consumer < clean)),
        "cleanup must follow the successful action identity check and final consumer: {job}"
    );
    assert!(
        job.contains("if: always() && steps.mbx-ready.outcome == 'success'"),
        "cleanup must require successful action setup without masking task failure: {job}"
    );
    if let Some(sample) = post_clean_sample {
        let clean = clean.unwrap_or(usize::MAX);
        assert!(
            job.find(&format!("      - name: {sample}\n"))
                .is_some_and(|sample| clean < sample),
            "the final disk sample must follow workspace cleanup: {job}"
        );
    }
}

#[cfg(unix)]
#[test]
fn reader_probes_fail_when_stats_producer_fails_after_valid_json() -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let repo = make_repo(&crate::impl_schema2_routing::workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification =
        crate::impl_schema2_routing::required_file(&tree, ".github/workflows/qualification.yml")?;
    let reader = crate::impl_schema2_routing::job_body(qualification, "mbx-cache-read-hosted")?;
    let scratch = TempDir::new()?;
    let bin = scratch.path().join("bin");
    fs::create_dir_all(&bin)?;
    let mbx = bin.join("mbx");
    fs::write(
        &mbx,
        "#!/bin/sh\nprintf '%s\\n' '{\"objects\":1,\"savings\":{\"cached_compilations\":1}}'\nexit 7\n",
    )?;
    fs::set_permissions(&mbx, fs::Permissions::from_mode(0o755))?;
    let jq = bin.join("jq");
    fs::write(&jq, "#!/bin/sh\nprintf invoked > \"$JQ_MARKER\"\nexit 0\n")?;
    fs::set_permissions(&jq, fs::Permissions::from_mode(0o755))?;
    let df = bin.join("df");
    fs::write(&df, "#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(&df, fs::Permissions::from_mode(0o755))?;
    let inherited_path = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
    let path = std::env::join_paths(
        std::iter::once(bin.clone()).chain(std::env::split_paths(&inherited_path)),
    )?;

    for (step, stats_file) in [
        ("Require imported MBX objects", "mbx-object-stats.json"),
        ("Require reused compilation", "mbx-reuse-stats.json"),
    ] {
        let run = rendered_step_run(reader, step)?;
        assert!(run.starts_with("set -e -o pipefail;"), "{run}");
        let runner_temp = scratch.path().join(step.replace(' ', "-"));
        fs::create_dir_all(&runner_temp)?;
        let marker = runner_temp.join("jq-was-called");
        let output = Command::new("/bin/bash")
            .arg("-e")
            .arg("-c")
            .arg(&run)
            .env("PATH", &path)
            .env("RUNNER_TEMP", &runner_temp)
            .env("JQ_MARKER", &marker)
            .output()?;
        assert!(
            !output.status.success(),
            "{} unexpectedly passed: {}{}",
            step,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(runner_temp.join(stats_file))?,
            "{\"objects\":1,\"savings\":{\"cached_compilations\":1}}\n"
        );
        assert!(
            !marker.exists(),
            "jq ran after {step} received a failing MBX producer"
        );
    }
    Ok(())
}

fn rendered_step_run(reader: &str, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let marker = format!("- name: {name}\n");
    let (_, after_step) = reader
        .split_once(&marker)
        .ok_or_else(|| format!("missing step {name}"))?;
    let run_line = after_step
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or_else(|| format!("missing run command for {name}"))?;
    Ok(serde_json::from_str(run_line)?)
}
