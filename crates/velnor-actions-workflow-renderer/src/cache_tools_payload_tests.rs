//! Read-only transports must precede the exact compiled bootstrap.
use super::*;
use velnor_actions_contract::JobTimeout;

fn setup() -> crate::MiseSetup {
    crate::setup::fixture::mise_setup("2026.9.16", &"a".repeat(64))
}
fn fixture() -> Job {
    Job {
        cache_mode: None,
        display_name: "Demo".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps: Vec::new(),
    }
}
fn ensure(job: &mut Job) -> Result<(), RenderError> {
    crate::cache_p08::ensure_setup_p08("demo", job, &setup(), true, "x86_64-unknown-linux-gnu", &[])
}
fn rendered() -> Job {
    let mut job = fixture();
    ensure(&mut job).expect("bootstrap");
    job
}

#[test]
fn repeated_render_preserves_exact_compiled_bootstrap() {
    let mut job = rendered();
    let steps = job.steps.clone();
    ensure(&mut job).expect("repeat");
    assert_eq!(job.steps, steps);
    assert!(
        matches!(&job.steps[2].kind, StepKind::SourceBoundHelper { invocation, .. }
        if invocation.descriptor().operation() == velnor_actions_contract::SourceBoundOperation::MiseBootstrap)
    );
}

#[test]
fn changed_duplicate_skipped_or_misordered_bootstraps_reject() {
    let job = rendered();
    let mut changed = job.clone();
    let StepKind::SourceBoundHelper { env, .. } = &mut changed.steps[2].kind else {
        panic!("bootstrap")
    };
    env.insert("VELNOR_MISE_SHA256".to_owned(), "b".repeat(64));
    assert!(ensure(&mut changed).is_err());
    let mut duplicate = job.clone();
    duplicate.steps.push(duplicate.steps[2].clone());
    assert!(ensure(&mut duplicate).is_err());
    let mut skipped = job.clone();
    skipped.steps[2].condition = Some("false".to_owned());
    assert!(ensure(&mut skipped).is_err());
    let mut late = job;
    late.steps.swap(1, 2);
    assert!(ensure(&mut late).is_err());
}

#[test]
fn altered_restore_or_unbound_platform_cannot_bypass_acquisition() {
    let mut altered = rendered();
    let StepKind::Action { with, .. } = &mut altered.steps[1].kind else {
        panic!("restore")
    };
    with.insert("path".to_owned(), "/tmp/foreign".to_owned());
    assert!(ensure(&mut altered).is_err());
    let mut missing = rendered();
    missing.steps.remove(0);
    assert!(ensure(&mut missing).is_err());
}

#[test]
fn renamed_duplicate_alias_restore_cannot_overwrite_verified_state() {
    for path in [
        "${{runner.temp}}/velnor/mise",
        "${{ runner.temp }}/velnor/foo/../mise",
        "$RUNNER_TEMP/velnor/mise",
        "${{ runner.temp }}/velnor/**",
    ] {
        let mut job = rendered();
        let mut restore = job.steps[1].clone();
        restore.name = "Renamed payload".to_owned();
        restore.id = None;
        let StepKind::Action { with, .. } = &mut restore.kind else {
            panic!("restore")
        };
        with.insert("path".to_owned(), path.to_owned());
        job.steps.push(restore);
        assert!(ensure(&mut job).is_err(), "alias {path}");
    }
}
