use super::Job;
use crate::workflow::cache_mode::validate_job;
use crate::workflow::pages::NativePagesTriggerPolicy;
use crate::workflow::timeout::JobTimeout;
use crate::workflow::{
    CacheMode, HelperInvocation, MbxCacheDomain, MbxExportDescriptor, MbxOwnerIdentity,
    NativePagesActions, NativePagesDeploy, ProducerAdmission, ProducerPolicy, ProducerRole,
    PureMbxProducer, PureToolProducer, SourceBoundHelper, SourceBoundOperation, SourceProducer,
    SourceProducerRole, StepId, ToolCacheDescriptor, ToolCacheDomain, ToolProducerSelection,
};
use std::collections::BTreeMap;

const TASK: &str = "stack/rust/default/check/default";

fn selection() -> ToolProducerSelection {
    ToolProducerSelection {
        tasks: vec![TASK.to_owned()],
        cargo_fallback: false,
        unconditional: false,
    }
}

fn mbx() -> PureMbxProducer {
    PureMbxProducer {
        descriptor: MbxExportDescriptor {
            domain: MbxCacheDomain::Validation,
            producer_job_id: "rust-cache".to_owned(),
            runs_on: "ubuntu-24.04".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            workspace_roots: vec![".".to_owned()],
            configuration_digest: crate::digest_b3(b"configuration"),
            task_digests: BTreeMap::from([(TASK.to_owned(), crate::digest_b3(b"task"))]),
            owner: MbxOwnerIdentity {
                version: "1.21.1-owned-cache-transport".to_owned(),
                binary_sha256: "a".repeat(64),
                qualification_identity: "b".repeat(64),
                source_sha: "c".repeat(40),
            },
            action_sha: "d".repeat(40),
        },
        selection: selection(),
        installation_step: StepId::new("mbx-install").expect("step"),
        admission_step: StepId::new("mbx-admit").expect("step"),
        verification_step: StepId::new("mbx-verify").expect("step"),
        save_step: StepId::new("mbx-save").expect("step"),
        publication_step: StepId::new("mbx-publication").expect("step"),
        report_step: StepId::new("mbx-report").expect("step"),
    }
}

fn source() -> SourceProducer {
    SourceProducer {
        role: SourceProducerRole::Cargo,
        selection: selection(),
        tool_cache: None,
        source_identity: "source-v1".to_owned(),
        verification_step: StepId::new("source-verify").expect("step"),
        restore_step: StepId::new("source-restore").expect("step"),
        save_step: StepId::new("source-save").expect("step"),
        publication_step: StepId::new("source-publication").expect("step"),
        report_step: StepId::new("source-report").expect("step"),
    }
}

fn tool() -> PureToolProducer {
    PureToolProducer {
        descriptor: ToolCacheDescriptor {
            domain: ToolCacheDomain::Full,
            target: "x86_64-unknown-linux-gnu".to_owned(),
            runs_on: "ubuntu-24.04".to_owned(),
            selectors: vec!["rust@1.98.1".to_owned()],
            immutable_identity: "qualified-tool".to_owned(),
            qualification_identity: format!("qualified-tools@{}", "e".repeat(64)),
        },
        selection: selection(),
        restore_step: StepId::new("tool-restore").expect("step"),
        before_step: StepId::new("tool-before").expect("step"),
        installation_step: StepId::new("tool-install").expect("step"),
        after_step: StepId::new("tool-after").expect("step"),
        save_step: StepId::new("tool-save").expect("step"),
        report_step: StepId::new("tool-report").expect("step"),
    }
}

fn job() -> Job {
    let producer = mbx();
    Job {
        cache_mode: Some(CacheMode::Write),
        display_name: "MBX writer".to_owned(),
        runs_on: producer.descriptor.runs_on.clone(),
        timeout_minutes: JobTimeout::CRATE,
        needs: producer.needs(),
        condition: Some(producer.condition()),
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: Some(producer),
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps: Vec::new(),
    }
}

fn privileged_role() -> NativePagesDeploy {
    let operation = SourceBoundOperation::NativePagesAdmission;
    let invocation = HelperInvocation::compiled(
        SourceBoundHelper::compiled(operation, operation.path(), &"a".repeat(64))
            .expect("descriptor"),
        Vec::new(),
        Vec::new(),
    )
    .expect("invocation");
    NativePagesDeploy {
        repository: "owner/repo".to_owned(),
        default_branch: "main".to_owned(),
        trigger_policy: NativePagesTriggerPolicy::ProtectedPushDispatch,
        preparation: Vec::new(),
        full_ci_job: "ci".to_owned(),
        full_ci_admission: invocation.clone(),
        artifact_job: "artifact".to_owned(),
        artifact_id_output: "id".to_owned(),
        artifact_digest_output: "digest".to_owned(),
        admission_step: StepId::new("admission").expect("step"),
        admission: invocation,
        admission_extra_environment: BTreeMap::new(),
        deploy_step: StepId::new("deploy").expect("step"),
        actions: NativePagesActions {
            checkout: "actions/checkout@".to_owned(),
            configure: "actions/configure-pages@".to_owned(),
            upload: "actions/upload-pages-artifact@".to_owned(),
            deploy: "actions/deploy-pages@".to_owned(),
        },
    }
}

#[test]
fn canonical_mbx_writer_owns_write_condition_and_binding() {
    let actual = job();
    assert!(actual.validate_producers().is_ok());
    assert!(validate_job(&actual).is_ok());
}

#[test]
fn foreign_mbx_condition_runner_needs_or_environment_is_rejected() {
    let mut wrong_condition = job();
    wrong_condition.condition = Some("always()".to_owned());
    assert!(wrong_condition.validate_producers().is_err());
    assert!(validate_job(&wrong_condition).is_err());

    let mut wrong_runner = job();
    wrong_runner.runs_on = "ubuntu-26.04".to_owned();
    assert!(wrong_runner.validate_producers().is_err());

    let mut wrong_needs = job();
    wrong_needs.needs = vec!["plan".to_owned()];
    assert!(wrong_needs.validate_producers().is_err());

    let mut wrong_environment = job();
    wrong_environment.environment = Some("release".to_owned());
    assert!(wrong_environment.validate_producers().is_err());

    let mut wrong_mode = job();
    wrong_mode.cache_mode = Some(CacheMode::WriteOnly);
    assert!(validate_job(&wrong_mode).is_err());
}

#[test]
fn producer_roles_and_privileged_roles_cannot_share_an_mbx_job() {
    let mut all_roles = job();
    all_roles.source_producer = Some(source());
    all_roles.tool_producer = Some(tool());
    assert!(all_roles.validate_producers().is_err());

    let mut source_conflict = job();
    source_conflict.source_producer = Some(source());
    assert!(source_conflict.validate_producers().is_err());

    let mut tool_conflict = job();
    tool_conflict.tool_producer = Some(tool());
    assert!(tool_conflict.validate_producers().is_err());

    let mut privileged = job();
    privileged.native_pages_deploy = Some(privileged_role());
    assert!(privileged.validate_producers().is_err());
}

#[test]
fn mbx_admission_uses_descriptor_identity_and_closed_job_id() {
    let producer = mbx();
    let mut admission = ProducerAdmission {
        job_id: String::new(),
        role: ProducerRole::Mbx {
            producer: producer.clone(),
        },
        policy: ProducerPolicy::AdvisoryFallback,
    };
    let expected = admission.expected_job_id().expect("expected job id");
    admission.job_id = expected.clone();
    assert_eq!(admission.job_id, expected);
    assert_eq!(
        admission.identity().expect("identity"),
        producer.descriptor.identity().expect("descriptor identity")
    );
    assert!(admission.validate_role().is_ok());
    admission.policy = ProducerPolicy::Mandatory;
    assert!(admission.validate_role().is_err());
    admission.policy = ProducerPolicy::AdvisoryFallback;

    admission.job_id = "mbx-validation-forged".to_owned();
    assert_ne!(
        admission.job_id,
        admission.expected_job_id().expect("closed job id")
    );
}

#[test]
fn cache_recipe_digest_binds_the_complete_mbx_job_and_one_role() {
    let original = job();
    let digest = crate::cache_producer_recipe_digest(&original).expect("MBX recipe digest");

    let mut renamed = original.clone();
    renamed.display_name.push_str(" changed");
    assert_ne!(
        digest,
        crate::cache_producer_recipe_digest(&renamed).expect("renamed digest")
    );

    let mut missing = original.clone();
    missing.mbx_producer = None;
    assert!(crate::cache_producer_recipe_digest(&missing).is_err());

    let mut duplicate = original.clone();
    duplicate.tool_producer = Some(tool());
    assert!(crate::cache_producer_recipe_digest(&duplicate).is_err());

    let mut privileged = original;
    privileged.native_pages_deploy = Some(privileged_role());
    assert!(crate::cache_producer_recipe_digest(&privileged).is_err());
}
