use super::named;
use crate::config::{
    GitHubEvidenceArtifactSource, GitHubEvidenceSnapshotInput, GitHubEvidenceWorkflowSource,
    MiseTaskSource, MiseTaskSuite, MiseTaskSuiteEvent, RustSuiteArtifact, RustSuiteArtifactWhen,
    VerificationRunner, WorkflowConfig,
};

fn suite(id: &str, events: Vec<MiseTaskSuiteEvent>) -> MiseTaskSuite {
    MiseTaskSuite {
        id: id.to_owned(),
        mise_task: format!("collect-{id}"),
        source: MiseTaskSource::default(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 15,
        workflow: "evidence".to_owned(),
        events,
        schedule: Vec::new(),
        artifacts: Vec::new(),
        evidence_snapshot: None,
    }
}

