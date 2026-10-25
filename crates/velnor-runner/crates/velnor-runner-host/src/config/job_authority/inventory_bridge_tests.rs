use std::collections::BTreeMap;

use super::*;

fn connection() -> RepositoryConnection {
    let connection = crate::config_test_support::basic_connection(
        "essential-mac",
        352_130_570,
        "donbeave/essential-mac",
        "essential-mac",
        "essential-mac",
    );
    let text = crate::config_test_support::host_config(&[connection], 1);
    crate::config::HostConfig::parse(&text)
        .expect("valid host config")
        .connections()[0]
        .clone()
}

fn job(role: JobAuthorityRole) -> InventoryJob {
    InventoryJob {
        root_path: ".github/workflows/ci.yml".to_owned(),
        workflow_path: ".github/workflows/ci.yml".to_owned(),
        job_key: "required".to_owned(),
        role,
        contexts: vec![InventoryContext {
            event: "push".to_owned(),
            branch_filters: vec!["sea/integration".to_owned()],
            tag_filters: Vec::new(),
            pull_request_base_branches: Vec::new(),
        }],
        permissions_declared: true,
        permissions: BTreeMap::from([("contents".to_owned(), "read".to_owned())]),
        secrets: Vec::new(),
        environments: Vec::new(),
        condition_sha256: None,
        concurrency: None,
        needs: Vec::new(),
    }
}

fn inventory(jobs: Vec<InventoryJob>) -> JobAuthorityInventory {
    let mut inventory = JobAuthorityInventory {
        schema_version: INVENTORY_SCHEMA,
        generator_source_sha: "a".repeat(40),
        inventory_sha256: String::new(),
        workflows: vec![InventoryWorkflow {
            path: ".github/workflows/ci.yml".to_owned(),
            content_sha256: "c".repeat(64),
            calls: Vec::new(),
            jobs,
        }],
    };
    let content = InventoryContent {
        schema_version: inventory.schema_version,
        generator_source_sha: &inventory.generator_source_sha,
        workflows: &inventory.workflows,
    };
    inventory.inventory_sha256 =
        sha256(&serde_json::to_vec(&content).expect("inventory content serializes"));
    inventory
}

fn build(value: &JobAuthorityInventory) -> Result<String, HostError> {
    policy_json_from_inventory(
        &connection(),
        &serde_json::to_string(value).expect("inventory serializes"),
        &"a".repeat(40),
        &"b".repeat(64),
        1,
    )
}

#[test]
fn converts_inventory_and_binds_policy_to_connection_and_generator() {
    let source = connection();
    let inventory = inventory(vec![job(JobAuthorityRole::Verification)]);
    let json = policy_json_from_inventory(
        &source,
        &serde_json::to_string(&inventory).expect("inventory serializes"),
        &inventory.generator_source_sha,
        &"b".repeat(64),
        7,
    )
    .expect("valid inventory converts");
    let policy: ApprovedJobAuthorityPolicy =
        serde_json::from_str(&json).expect("policy JSON parses");
    policy.validate_for(&source).expect("policy validates");
    assert_eq!(policy.generation(), 7);
    assert_eq!(policy.connection_id(), &source.connection_id);
    assert_eq!(policy.workflows[0].repository_id, 352_130_570);
    assert_eq!(
        policy.workflows[0].jobs[0].contexts[0].refs,
        ["refs/heads/sea/integration"]
    );

    let host_text = crate::config_test_support::host_config(
        &[crate::config_test_support::basic_connection(
            "essential-mac",
            352_130_570,
            "donbeave/essential-mac",
            "essential-mac",
            "essential-mac",
        )],
        1,
    );
    let host = crate::config::HostConfig::parse(&host_text).expect("valid host config");
    let imported = host
        .with_job_authority_inventory(
            &source.connection_id,
            &serde_json::to_string(&inventory).expect("inventory serializes"),
            &inventory.generator_source_sha,
            &"b".repeat(64),
            7,
        )
        .expect("inventory binds through HostConfig validation");
    assert!(
        imported
            .validated_job_authority_policy(&source.connection_id)
            .expect("policy lookup validates config")
            .is_some()
    );

    let mut other_repository = source.clone();
    other_repository.repository_id += 1;
    assert!(policy.validate_for(&other_repository).is_err());
}

#[test]
fn rejects_inventory_digest_source_and_shape_mismatch() {
    let good = inventory(vec![job(JobAuthorityRole::Verification)]);
    let mut changed = good;
    changed.workflows[0].content_sha256 = "d".repeat(64);
    assert!(build(&changed).is_err());

    let good = inventory(vec![job(JobAuthorityRole::Verification)]);
    let text = serde_json::to_string(&good).expect("inventory serializes");
    assert!(
        policy_json_from_inventory(&connection(), &text, &"e".repeat(40), &"b".repeat(64), 1,)
            .is_err()
    );
    let unknown_field = text.replace(
        "\"schema_version\":1",
        "\"future\":true,\"schema_version\":1",
    );
    assert!(
        policy_json_from_inventory(
            &connection(),
            &unknown_field,
            &good.generator_source_sha,
            &"b".repeat(64),
            1,
        )
        .is_err()
    );
}

#[test]
fn rejects_contexts_that_are_not_a_finite_source_policy_intersection() {
    let mut value = inventory(vec![job(JobAuthorityRole::Verification)]);
    value.workflows[0].jobs[0].contexts[0].branch_filters = vec!["release-*".to_owned()];
    reseal(&mut value);
    assert!(build(&value).is_err());

    let mut value = inventory(vec![job(JobAuthorityRole::Verification)]);
    value.workflows[0].jobs[0].contexts[0].event = "release".to_owned();
    value.workflows[0].jobs[0].contexts[0]
        .branch_filters
        .clear();
    reseal(&mut value);
    assert!(build(&value).is_err());

    let mut value = inventory(vec![job(JobAuthorityRole::Verification)]);
    value.workflows[0].jobs[0].contexts[0].event = "schedule".to_owned();
    value.workflows[0].jobs[0].contexts[0]
        .branch_filters
        .clear();
    reseal(&mut value);
    assert!(build(&value).is_err());
}

#[test]
fn host_validator_rejects_unreviewed_publication_authority() {
    let mut value = inventory(vec![job(JobAuthorityRole::Verification)]);
    let verification = &mut value.workflows[0].jobs[0];
    verification
        .permissions
        .insert("contents".to_owned(), "write".to_owned());
    verification.secrets.push("PUBLISH_TOKEN".to_owned());
    reseal(&mut value);
    assert!(build(&value).is_err());
}

#[test]
fn derives_only_pull_request_rules_with_an_allowed_base_branch() {
    let config = crate::config_test_support::connection(
        "essential-mac",
        352_130_570,
        "donbeave/essential-mac",
        "essential-mac",
        "essential-mac",
        "branches = [\"main\"]\ntag_prefixes = []\nevents = [\"pull_request\"]\npull_request_rules = [{ number = 17, base_repository_id = 352130570, base_branch = \"main\", head_repository_id = 352130570, head_branch = \"feat/policy\" }]",
    );
    let host =
        crate::config::HostConfig::parse(&crate::config_test_support::host_config(&[config], 1))
            .expect("PR host config");
    let source = &host.connections()[0];
    let mut value = inventory(vec![job(JobAuthorityRole::Verification)]);
    let context = &mut value.workflows[0].jobs[0].contexts[0];
    context.event = "pull_request".to_owned();
    context.branch_filters.clear();
    context.pull_request_base_branches = vec!["main".to_owned()];
    reseal(&mut value);
    let json = policy_json_from_inventory(
        source,
        &serde_json::to_string(&value).expect("inventory serializes"),
        &value.generator_source_sha,
        &"b".repeat(64),
        1,
    )
    .expect("source-limited PR context converts");
    let policy: ApprovedJobAuthorityPolicy =
        serde_json::from_str(&json).expect("policy JSON parses");
    assert_eq!(
        policy.workflows[0].jobs[0].contexts[0].pull_request_rules,
        [crate::config::PullRequestRule {
            number: Some(17),
            base_repository_id: 352_130_570,
            base_branch: "main".to_owned(),
            head_repository_id: 352_130_570,
            head_branch: "feat/policy".to_owned(),
        }]
    );
}

fn reseal(value: &mut JobAuthorityInventory) {
    let content = InventoryContent {
        schema_version: value.schema_version,
        generator_source_sha: &value.generator_source_sha,
        workflows: &value.workflows,
    };
    value.inventory_sha256 =
        sha256(&serde_json::to_vec(&content).expect("inventory content serializes"));
}
