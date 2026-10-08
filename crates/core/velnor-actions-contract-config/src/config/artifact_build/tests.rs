use super::{ArtifactBuildOutput, ArtifactBuildTask};
use crate::config::VerificationRunner;

fn output(id: &str, path: &str, max_bytes: u64) -> ArtifactBuildOutput {
    ArtifactBuildOutput {
        id: id.to_owned(),
        path: path.to_owned(),
        max_bytes,
    }
}

fn task() -> ArtifactBuildTask {
    ArtifactBuildTask {
        id: "frontend-bundle".to_owned(),
        mise_task: "build-frontend".to_owned(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 30,
        outputs: vec![output("bundle", "dist/app.tar", 16_384)],
    }
}

#[test]
fn accepts_explicit_nonempty_bounded_file_outputs() {
    assert!(task().validate("config.toml").is_ok());
}

#[test]
fn rejects_unbounded_or_ambiguous_file_outputs() {
    let mut candidate = task();
    candidate.outputs[0].max_bytes = 0;
    assert!(
        candidate
            .validate("config.toml")
            .expect_err("zero bound fails")
            .to_string()
            .contains("must_be_positive")
    );

    for path in ["/tmp/output", "dist/../secret", "dist/*.tar", "dist//x"] {
        let mut candidate = task();
        candidate.outputs[0].path = path.to_owned();
        assert!(
            candidate
                .validate("config.toml")
                .expect_err("unsafe path fails")
                .to_string()
                .contains("unsafe_artifact_path")
        );
    }

    let mut candidate = task();
    candidate
        .outputs
        .push(output("copy", "dist/app.tar", 16_384));
    assert!(
        candidate
            .validate("config.toml")
            .expect_err("duplicate path fails")
            .to_string()
            .contains("duplicate_artifact_output_path")
    );
}

#[test]
fn artifact_builds_are_linux_only_and_require_sorted_unique_ids() {
    let mut candidate = task();
    candidate.runner = VerificationRunner::MacosArm64;
    assert!(
        candidate
            .validate("config.toml")
            .expect_err("unsupported platform fails")
            .to_string()
            .contains("artifact_build_requires_linux_x64")
    );

    let mut candidate = task();
    candidate.outputs = vec![
        output("zeta", "dist/zeta", 10),
        output("alpha", "dist/alpha", 10),
    ];
    assert!(
        candidate
            .validate("config.toml")
            .expect_err("unsorted inventory fails")
            .to_string()
            .contains("artifact_outputs_must_be_sorted_by_id")
    );
}
