//! Gate-6 reuse cases: qualification, keys, transport, planning.
use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_mise::cache::{CachedTaskDescriptor, TaskCacheMode};
use velnor_actions_mise::restore::{MissReason, ToolAvailability};
use velnor_actions_mise::reuse::{
    ReuseGrant, ReuseQualification, ReuseSignal, TaskArtifactTransport, TaskCacheKey,
    TaskReuseRequest,
};
use velnor_actions_mise::reuse::{ReusePlan, plan_reuse};
use velnor_actions_mise::{Gate6Fixture, MiseError};

fn descriptor(task: &str) -> CachedTaskDescriptor {
    CachedTaskDescriptor {
        task_name: task.to_owned(),
        sources: vec!["src/**/*.rs".to_owned()],
        outputs: vec!["target/debug/app".to_owned()],
        command_inputs: vec!["rustc --version".to_owned()],
        env: BTreeMap::new(),
        tools: vec!["rust@1.98.1".to_owned()],
        dep_keys: Vec::new(),
    }
}

fn grant_for(kind: &str, event: &str, mode: TaskCacheMode) -> Result<ReuseGrant, String> {
    ReuseQualification::new(kind, event)
        .check(mode)
        .map_err(|err| err.to_string())
}

/// REUSE-1: publishing kinds, nondeterminism, and undeclared state never
/// qualify and always run.
#[test]
fn qualification_rejects_nondeterministic_and_undeclared_state() {
    for kind in ["publish", "deploy", "notify", "service"] {
        let qualification = ReuseQualification::new(kind, "push");
        assert!(qualification.always_run(), "{kind} must always run");
        assert!(qualification.check(TaskCacheMode::ReadWrite).is_err());
    }
    for signal in [
        ReuseSignal::Network,
        ReuseSignal::Clock,
        ReuseSignal::Random,
        ReuseSignal::UndeclaredState,
    ] {
        let qualification = ReuseQualification::new("clippy", "push").with_signal(signal);
        assert!(qualification.always_run(), "{signal:?} must always run");
        assert!(qualification.check(TaskCacheMode::ReadWrite).is_err());
    }
    let clean = ReuseQualification::new("clippy", "push");
    assert!(!clean.always_run());
    let grant = clean
        .check(TaskCacheMode::ReadOnly)
        .expect("clean qualifies");
    assert_eq!(grant.task(), "clippy");
    assert_eq!(grant.mode(), TaskCacheMode::ReadOnly);
}

/// REUSE-1/3/4/7: release events and `Off` mode never reuse.
#[test]
fn release_events_never_reuse() {
    let release = ReuseQualification::new("clippy", "release");
    let err = release
        .check(TaskCacheMode::ReadWrite)
        .expect_err("release must not grant");
    assert_eq!(
        err,
        MiseError::CacheNotEligible {
            task: "clippy".to_owned(),
            reason: "forced_uncached".to_owned(),
        }
    );
    assert!(
        ReuseQualification::new("clippy", "push")
            .check(TaskCacheMode::Off)
            .is_err(),
        "Off mode must not grant"
    );
    let fixture = Gate6Fixture::new("gate6/clippy-cache").expect("fixture");
    assert!(
        TaskArtifactTransport::open(&fixture, TaskCacheMode::Off, Path::new("/tmp/cache")).is_err(),
        "Off mode must not open transport"
    );
    assert!(
        matches!(
            plan_reuse(
                ToolAvailability::Ready,
                &ReuseQualification::new("clippy", "release"),
                TaskCacheMode::ReadWrite,
            ),
            ReusePlan::Execute(fallback)
                if fallback.reason() == MissReason::FORCED_UNCACHED
        ),
        "release must plan Execute(forced_uncached)"
    );
}

/// REUSE-2: undeclared inputs fail before execution; qualified requests
/// reach the fixed run argv.
#[test]
fn reuse_request_rejects_undeclared_inputs_before_execution() {
    let grant = grant_for("clippy", "push", TaskCacheMode::ReadOnly).expect("grant");
    let undeclared = vec!["/etc/shadow".to_owned()];
    assert!(
        TaskReuseRequest::new(descriptor("clippy"), &undeclared, grant.clone()).is_err(),
        "undeclared reads must fail"
    );
    assert!(
        TaskReuseRequest::new(descriptor("other"), &[], grant.clone()).is_err(),
        "grant/descriptor mismatch must fail"
    );
    let sourceless = CachedTaskDescriptor {
        sources: Vec::new(),
        ..descriptor("clippy")
    };
    assert!(
        TaskReuseRequest::new(sourceless, &[], grant.clone()).is_err(),
        "sourceless descriptor must fail"
    );
    let request =
        TaskReuseRequest::new(descriptor("clippy"), &[], grant).expect("qualified request builds");
    assert_eq!(
        request
            .run_argv("clippy", "$RUNNER_TEMP/velnor/tasks/clippy.toml")
            .expect("run argv"),
        [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "run",
            "--task-cache",
            "read-only",
            "clippy",
            "--file",
            "$RUNNER_TEMP/velnor/tasks/clippy.toml",
        ]
    );
}

/// REUSE-3: key derivation needs a grant naming the descriptor task and is
/// deterministic over declared inputs.
#[test]
fn cache_key_derivation_requires_matching_grant() {
    let grant = grant_for("clippy", "push", TaskCacheMode::ReadOnly).expect("grant");
    assert!(
        TaskCacheKey::derive(&grant, &descriptor("other")).is_err(),
        "mismatched grant must not derive"
    );
    let first = TaskCacheKey::derive(&grant, &descriptor("clippy")).expect("derive");
    let second = TaskCacheKey::derive(&grant, &descriptor("clippy")).expect("derive");
    assert_eq!(first, second, "derivation must be deterministic");
    assert!(
        first.digest().starts_with("b3-"),
        "b3 digest: {}",
        first.digest()
    );
    let changed = CachedTaskDescriptor {
        sources: vec!["src/other.rs".to_owned()],
        ..descriptor("clippy")
    };
    let third = TaskCacheKey::derive(&grant, &changed).expect("derive");
    assert_ne!(first.digest(), third.digest(), "inputs feed the key");
}

/// REUSE-4: transport is opaque paths under `task-artifacts/v2`, escapes
/// refused, `Off` refused.
#[test]
fn artifact_transport_stays_opaque_and_rooted() {
    let fixture = Gate6Fixture::new("gate6/clippy-cache").expect("fixture");
    let transport =
        TaskArtifactTransport::open(&fixture, TaskCacheMode::ReadOnly, Path::new("/tmp/cache"))
            .expect("transport opens");
    assert_eq!(
        transport.root(),
        Path::new("/tmp/cache/task-artifacts/v2"),
        "fixed layout suffix"
    );
    assert_eq!(transport.mode(), TaskCacheMode::ReadOnly);
    assert_eq!(
        transport.resolve("clippy/out.json").expect("resolve"),
        Path::new("/tmp/cache/task-artifacts/v2/clippy/out.json")
    );
    for bad in ["", "/abs/path", "../escape", "a/../../escape"] {
        assert!(transport.resolve(bad).is_err(), "{bad} must not resolve");
    }
}

/// REUSE-7: missing tools execute without reuse and report
/// `cache_unavailable`; ready tools follow qualification.
#[test]
fn missing_tools_execute_with_cache_unavailable() {
    let qualified = ReuseQualification::new("clippy", "push");
    assert!(
        matches!(
            plan_reuse(ToolAvailability::Missing, &qualified, TaskCacheMode::ReadOnly),
            ReusePlan::Execute(fallback)
                if fallback.reason() == MissReason::CACHE_UNAVAILABLE
        ),
        "missing tools execute + cache_unavailable"
    );
    assert!(
        matches!(
            plan_reuse(ToolAvailability::Ready, &qualified, TaskCacheMode::ReadOnly),
            ReusePlan::Reuse(grant) if grant.task() == "clippy"
        ),
        "ready + qualified reuses"
    );
    let unqualified = ReuseQualification::new("publish", "push");
    assert!(
        matches!(
            plan_reuse(ToolAvailability::Ready, &unqualified, TaskCacheMode::ReadOnly),
            ReusePlan::Execute(fallback)
                if fallback.reason() == MissReason::TASK_NOT_ELIGIBLE
        ),
        "ready + unqualified executes + task_not_eligible"
    );
}
