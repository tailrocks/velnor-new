//! Provider routing and Required dependencies for declared artifact tasks.

use velnor_actions_orchestrator_generation::generate::render_staged_tree;
use velnor_actions_orchestrator_generation::prepare::prepare;

use crate::impl_common::{TestResult, git, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

const HOSTED_RUNS: &str = "runs-on: ubuntu-26.04";
const SCALE_RUNS: &str = "runs-on: [velnor, ubuntu-26.04-scale-set, verification-worker]";

#[test]
fn artifact_build_tasks_follow_each_execution_mode_and_gate_required() -> TestResult {
    for mode in ["hosted", "scale-set", "both"] {
        let repo = make_repo(&config(mode))?;
        git(
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/tailrocks/velnor-new.git",
            ],
            repo.path(),
        )?;
        git(&["add", "."], repo.path())?;
        git(&["commit", "-m", "artifact task fixture"], repo.path())?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let ci = required_file(&tree, ".github/workflows/ci.yml")?;
        let required = job_body(ci, "required")?;
        let plan = job_body(ci, "plan")?;

        assert!(required.contains("artifact-build"), "{mode}: {required}");

        match mode {
            "hosted" => {
                let artifact = job_body(ci, "artifact-build")?;
                assert!(plan.contains("artifact_hosted_matrix"), "{plan}");
                assert!(!plan.contains("artifact_velnor_matrix"), "{plan}");
                assert!(artifact.contains(HOSTED_RUNS), "{artifact}");
                assert!(!artifact.contains("ubuntu-26.04-scale-set"), "{artifact}");
                assert!(required.contains("- artifact-build"), "{required}");
                assert!(!ci.contains("artifact-build__hosted:"), "{ci}");
                assert!(!ci.contains("artifact-build__local:"), "{ci}");
            }
            "scale-set" => {
                let artifact = job_body(ci, "artifact-build")?;
                assert!(plan.contains("artifact_velnor_matrix"), "{plan}");
                assert!(!plan.contains("artifact_hosted_matrix"), "{plan}");
                assert!(artifact.contains(SCALE_RUNS), "{artifact}");
                assert!(required.contains("- artifact-build"), "{required}");
                assert!(!artifact.contains("runs-on: ubuntu-26.04\n"), "{artifact}");
                assert!(!ci.contains("artifact-build__hosted:"), "{ci}");
                assert!(!ci.contains("artifact-build__local:"), "{ci}");
            }
            "both" => {
                let hosted = job_body(ci, "artifact-build__hosted")?;
                let scale = job_body(ci, "artifact-build__local")?;
                assert!(plan.contains("artifact_hosted_matrix"), "{plan}");
                assert!(plan.contains("artifact_velnor_matrix"), "{plan}");
                assert!(hosted.contains(HOSTED_RUNS), "{hosted}");
                assert!(scale.contains(SCALE_RUNS), "{scale}");
                assert!(required.contains("- artifact-build__hosted"), "{required}");
                assert!(required.contains("- artifact-build__local"), "{required}");
                assert!(!ci.contains("  artifact-build:\n"), "{ci}");
            }
            _ => unreachable!(),
        }
        let producer = if mode == "both" {
            required_file(&tree, ".github/actions/artifact-build/action.yml")?
        } else {
            job_body(ci, "artifact-build")?
        };
        for expected in [
            "Capture declared artifact outputs",
            "VELNOR_INTERNAL_OP: export-artifact-v1",
            "VELNOR_ARTIFACT_TASK_ID: ${{ matrix.task_id }}",
            "VELNOR_ARTIFACT_PROVIDER: ${{ matrix.provider }}",
            "VELNOR_ARTIFACT_SOURCE_SHA: ${{ matrix.source_sha }}",
            "VELNOR_ARTIFACT_PLAN_DIGEST: ${{ matrix.plan_digest }}",
            "Upload verified build outputs",
            "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9",
            "name: ${{ matrix.artifact_name }}",
            "artifact-builds/${{ matrix.artifact_name }}",
            "if-no-files-found: error",
        ] {
            assert!(
                producer.contains(expected),
                "{mode}: missing {expected}: {producer}"
            );
        }
        let export_step = producer
            .split("name: Capture declared artifact outputs")
            .nth(1)
            .and_then(|steps| steps.split("name: Upload verified build outputs").next())
            .expect("export step and upload step");
        assert!(
            !export_step.contains("VELNOR_REQUEST_FILE"),
            "{mode}: {export_step}"
        );
        assert!(!export_step.contains("GH_TOKEN"), "{mode}: {export_step}");
        assert!(
            !export_step.contains("GITHUB_TOKEN"),
            "{mode}: {export_step}"
        );
        assert!(ci.contains("fail-fast: false"), "{mode}: {ci}");
        assert!(
            ci.contains("fromJSON(needs.plan.outputs.artifact_"),
            "{mode}: {ci}"
        );
        assert!(
            ci.contains("timeout-minutes: ${{ matrix.timeout_minutes }}"),
            "{mode}: {ci}"
        );
        match mode {
            "both" => {
                let action = required_file(&tree, ".github/actions/artifact-build/action.yml")?;
                assert!(
                    action.contains("${{ matrix.mise_task }}"),
                    "{mode}: {action}"
                );
            }
            "hosted" | "scale-set" => {
                assert!(ci.contains("${{ matrix.mise_task }}"), "{mode}: {ci}");
            }
            _ => unreachable!(),
        }
    }
    Ok(())
}

fn config(mode: &str) -> String {
    let base = format!(
        "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\npolicy = \"velnor-repository-v1\"\n\
         [[workflow.artifact_tasks]]\nid = \"frontend-bundle\"\nmise_task = \"build-frontend\"\nrunner = \"linux-x64\"\ntimeout_minutes = 30\n\
         [[workflow.artifact_tasks.outputs]]\nid = \"bundle\"\npath = \"dist/app.tar\"\nmax_bytes = 16384\n\
         [execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"{mode}\"\n\
         [execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
         [execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\", \"verification-worker\"]\nplatform = \"linux/amd64\"\n\
         [stacks.rust.policy]\nversion = \"0.1.3\"\nsha256 = \"104c0d8b3a827875776358f941aa88f1c5837c1009305076af9380f4e3fcda25\"\nprofile = \"rust-strict-v1\""
    );
    base
}
