#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(crate) enum FixtureScenario {
    #[default]
    Complete,
    OmitPredecessor,
    OmitProducerRun,
    ExtraAttemptArtifact,
    PartialWorkflowPage,
    WrongJobRunId,
    WrongJobHeadSha,
    MissingJobRunId,
    MissingJobHeadSha,
    MultiPageJobs,
    TruncatedJobsSecondPage,
    DuplicateJobId,
    WrongTaskJobName,
    BadConclusion,
    InvalidArchive,
}

#[derive(Default)]
pub(crate) struct FixtureReader {
    pub(super) scenario: FixtureScenario,
    pub(super) omit_attempt_artifact: Option<u32>,
}
