//! Opaque execution cannot enter the generated prefix, regardless of spelling.

use super::*;
use velnor_actions_contract::{
    HelperInvocation, JobTimeout, SourceBoundHelper, compiled_source_sha256,
};

const STAGED: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0";
const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

struct Fixture {
    job: Job,
    controls: [Step; 3],
    record: CompiledSourceHelper,
}

fn planning_record() -> CompiledSourceHelper {
    let selectors = [
        "actionlint@1.7.12",
        "gh@2.102.0",
        "shellcheck@0.11.0",
        "zizmor@1.30.1",
    ]
    .map(str::to_owned)
    .to_vec();
    let source = crate::marker::with_marker("0.1.0", "#!/bin/sh\nexit 0\n").expect("marker");
    let operation = SourceBoundOperation::MiseToolPrepare;
    let descriptor = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &compiled_source_sha256(source.as_bytes()),
    )
    .expect("descriptor");
    let mut args = vec![
        "planning".to_owned(),
        "x86_64-unknown-linux-gnu".to_owned(),
        "configuration".to_owned(),
    ];
    args.extend(selectors.clone());
    let invocation = HelperInvocation::compiled(descriptor, args, selectors).expect("invocation");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("compiled")
        .with_environment(BTreeMap::from([(
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::Planning.root().to_owned(),
        )]))
}

fn fixture() -> Fixture {
    let record = planning_record();
    let mut checkout = steps::checkout_step(CHECKOUT).expect("checkout");
    if let StepKind::Action { with, .. } = &mut checkout.kind {
        with.insert("fetch-depth".to_owned(), "0".to_owned());
    }
    let env = BTreeMap::from([
        (steps::ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
        (
            steps::ASSET_URL_ENV.to_owned(),
            "https://example.invalid/asset".to_owned(),
        ),
        (steps::RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
    ]);
    let mut acquire = steps::acquire_velnor_step(STAGED, &env).expect("acquire");
    if let StepKind::Shell { env, .. } = &mut acquire.kind {
        env.insert(
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::Planning.root().to_owned(),
        );
    }
    let platform = crate::cache_p08::payload::platform_step().expect("platform");
    let restore = crate::cache_steps::tools_restore_step("key").expect("restore");
    let setup = crate::setup::fixture::mise_setup("2026.9.16", &"a".repeat(64));
    let bootstrap =
        crate::setup::mise_setup_step(&setup, ToolCacheDomain::Planning, "ubuntu-24.04")
            .expect("bootstrap");
    let controls = [platform, restore, bootstrap];
    let mut ordered = vec![checkout, acquire];
    ordered.extend(controls.clone());
    ordered.push(
        crate::source_helper::source_helper_step(
            "Prepare planning tools",
            &record,
            record.environment().clone(),
        )
        .expect("prepare"),
    );
    ordered.push(steps::write_request_step(steps::PLAN_OPERATION).expect("request"));
    ordered.push(crate::early_plan::early_plan_step().expect("early"));
    let job = Job {
        cache_mode: None,
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps: ordered,
    };
    Fixture {
        job,
        controls,
        record,
    }
}

fn admitted(fixture: &Fixture) -> bool {
    validate(
        &fixture.job,
        STAGED,
        CHECKOUT,
        &fixture.controls[0],
        &fixture.controls[1],
        &fixture.controls[2],
        std::slice::from_ref(&fixture.record),
    )
    .is_ok()
}

#[test]
fn canonical_owner_prefix_admitted_and_registry_required() {
    let fixture = fixture();
    assert!(admitted(&fixture));
    assert!(
        validate(
            &fixture.job,
            STAGED,
            CHECKOUT,
            &fixture.controls[0],
            &fixture.controls[1],
            &fixture.controls[2],
            &[]
        )
        .is_err()
    );
}

#[test]
fn opaque_shell_hidden_cargo_path_override_remote_action_and_repo_task_rejected() {
    let shells = [
        vec![
            "sh",
            "-c",
            "exec \"${FULL_MISE}\" exec rust -- cargo metadata",
        ],
        vec![
            "sh",
            "-c",
            "printf '%s\\n' /tmp/attacker >> \"$GITHUB_PATH\"",
        ],
        vec!["mise", "run", "repository-task"],
    ];
    for argv in shells {
        let mut fixture = fixture();
        fixture.job.steps.insert(
            6,
            Step {
                id: None,
                name: "Write request".to_owned(),
                condition: None,
                kind: StepKind::Shell {
                    run: argv.into_iter().map(str::to_owned).collect(),
                    env: BTreeMap::new(),
                },
            },
        );
        assert!(!admitted(&fixture));
    }
    let mut fixture = fixture();
    fixture.job.steps.insert(
        6,
        Step {
            id: None,
            name: "Restore planning tools".to_owned(),
            condition: None,
            kind: StepKind::Action {
                uses: format!("attacker/run@{}", "a".repeat(40)),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        },
    );
    assert!(!admitted(&fixture));
}

#[test]
fn canonical_slots_cannot_hide_commands_env_conditions_or_foreign_helpers() {
    for script in [
        "sh -c \"${FULL_CARGO}\"",
        "printf '%s\\n' /tmp/attacker >> \"$GITHUB_PATH\"",
    ] {
        let mut case = fixture();
        if let StepKind::Shell { run, .. } = &mut case.job.steps[1].kind {
            run[2].push_str(" && ");
            run[2].push_str(script);
        }
        assert!(!admitted(&case));
    }
    for slot in [1, 5] {
        let mut case = fixture();
        case.job.steps[slot].kind = StepKind::Shell {
            run: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                "eval \"$HIDDEN\"".to_owned(),
            ],
            env: BTreeMap::new(),
        };
        assert!(!admitted(&case));
    }
    let mut case = fixture();
    if let StepKind::SourceBoundHelper { env, .. } = &mut case.job.steps[5].kind {
        env.insert("PATH".to_owned(), "/tmp/attacker".to_owned());
    }
    assert!(!admitted(&case));
    let mut case = fixture();
    case.job.steps[5].condition = Some("false".to_owned());
    assert!(!admitted(&case));
    let mut case = fixture();
    case.job.steps[6] = steps::plan_step();
    assert!(!admitted(&case));
}

#[test]
fn acquisition_path_and_planning_footprint_are_closed() {
    for suffix in ["", "0.1.0\";cargo", "$(cargo)", "../cargo", "x\n"] {
        assert!(acquisition_argv(&format!("{}{suffix}", steps::STAGED_BINARY_PREFIX)).is_err());
    }
    assert!(planning_footprint(
        planning_record().invocation().installed_selectors()
    ));
    let mut selectors = planning_record()
        .invocation()
        .installed_selectors()
        .to_vec();
    selectors.push("rust".to_owned());
    assert!(!planning_footprint(&selectors));
    assert_eq!(selectors.pop().as_deref(), Some("rust"));
    selectors[0] = "actionlint@".to_owned();
    assert!(!planning_footprint(&selectors));
}
