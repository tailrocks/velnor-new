// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[expect(
    clippy::duplicate_mod,
    reason = "T40 pins the shared unknown-gap reporting contract alongside its owning task test"
)]
#[path = "../src/compiler_observation.rs"]
mod compiler_observation;

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::time::Duration;

use self::compiler_observation::{
    ChildOutcome, ObservationReport, ObservationSession, new_session_id,
};
use velnor_actions_contract::config::{
    MiseTaskSource, RustBinaryReleaseConfig, RustBinaryReleaseProducer,
};
use velnor_actions_contract::package_release::PackageReleaseChannel;
use velnor_actions_mise::CheckDeadline;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn closure_source() -> &'static str {
    "[tasks.prep]\n\
     run = \"echo prep\"\n\
     [tasks.prep.tools]\n\
     node = \"22.14.0\"\n\
     [tasks.compile]\n\
     run = \"echo compile\"\n\
     depends = [\"prep\"]\n\
     [tasks.compile.tools]\n\
     rust = \"1.82.0\"\n\
     [tasks.link]\n\
     run = \"mise run compile\"\n\
     depends = [\"compile\"]\n\
     [tasks.build-preview]\n\
     run = \"echo build\"\n\
     depends = [\"compile\", \"link\"]\n\
     [tasks.build-preview.tools]\n\
     node = \"22.14.0\"\n"
}

fn resolve(task: &str, source: &str) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    crate::native_mise_tasks::resolve_preview_producer_tools(source, task).map_err(fail)
}

fn check_closure_union() -> Result<(), Box<dyn Error>> {
    let tools = resolve("build-preview", closure_source())?;
    let mut expected = BTreeMap::new();
    expected.insert("node".to_owned(), "22.14.0".to_owned());
    expected.insert("rust".to_owned(), "1.82.0".to_owned());
    if tools != expected {
        return Err(fail("preview closure missed native tool identities"));
    }
    Ok(())
}

fn check_closure_bound() -> Result<(), Box<dyn Error>> {
    let mut source = String::new();
    for index in 0..200 {
        std::fmt::Write::write_fmt(
            &mut source,
            format_args!(
                "[tasks.task-{index:03}]\nrun = \"echo {index}\"\ndepends = [\"task-{:03}\"]\n",
                index + 1
            ),
        )
        .map_err(io::Error::other)?;
    }
    source.push_str("[tasks.task-200]\nrun = \"echo last\"\n");
    if resolve("task-000", &source).is_ok() {
        return Err(fail("preview closure ignored its bound"));
    }
    Ok(())
}

fn check_closure_rejects() -> Result<(), Box<dyn Error>> {
    if resolve("build-preview", "[tasks").is_ok() {
        return Err(fail("preview closure accepted invalid source"));
    }
    if resolve("ghost", closure_source()).is_ok() {
        return Err(fail("preview closure accepted a missing task"));
    }
    if resolve("has space", closure_source()).is_ok() {
        return Err(fail("preview closure accepted an invalid name"));
    }
    let cycle = "[tasks.a]\nrun = \"echo a\"\ndepends = [\"b\"]\n[tasks.b]\nrun = \"echo b\"\ndepends = [\"a\"]\n";
    if resolve("a", cycle).is_ok() {
        return Err(fail("preview closure accepted a cycle"));
    }
    let conflict = "[tasks.a]\nrun = \"echo a\"\n[tasks.a.tools]\nnode = \"1.0.0\"\n\
        [tasks.b]\nrun = \"echo b\"\n[tasks.b.tools]\nnode = \"2.0.0\"\n\
        [tasks.root]\nrun = \"echo root\"\ndepends = [\"a\", \"b\"]\n";
    if resolve("root", conflict).is_ok() {
        return Err(fail("preview closure accepted conflicting pins"));
    }
    let escape = "[tasks.root]\nrun = \"mise run --force ghost\"\n";
    if resolve("root", escape).is_ok() {
        return Err(fail("preview closure accepted a nested escape"));
    }
    let missing_nested = "[tasks.root]\nrun = \"mise run ghost\"\n";
    if resolve("root", missing_nested).is_ok() {
        return Err(fail("preview closure accepted a missing nested task"));
    }
    let unpinned = "[tasks.root]\nrun = \"echo root\"\n[tasks.root.tools]\nnode = \"latest\"\n";
    if resolve("root", unpinned).is_ok() {
        return Err(fail("preview closure accepted an unpinned tool"));
    }
    let missing_dep = "[tasks.root]\nrun = \"echo root\"\ndepends = [\"ghost\"]\n";
    if resolve("root", missing_dep).is_ok() {
        return Err(fail("preview closure accepted a missing dependency"));
    }
    Ok(())
}

fn producer(task: &str, config: &str, working: &str) -> RustBinaryReleaseProducer {
    RustBinaryReleaseProducer {
        task: task.to_owned(),
        source: MiseTaskSource {
            mise_config: config.to_owned(),
            working_directory: working.to_owned(),
        },
        ..Default::default()
    }
}

fn deadline() -> Result<CheckDeadline, Box<dyn Error>> {
    Ok(CheckDeadline::after(Duration::from_secs(120))?)
}

fn check_discovery_rejects() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let deadline = deadline()?;
    if velnor_actions_mise::discover_preview_producer(
        root,
        &producer("build-preview", "../evil.toml", "."),
        &[],
        deadline,
    )
    .is_ok()
    {
        return Err(fail("preview discovery accepted a bad source path"));
    }
    if velnor_actions_mise::discover_preview_producer(
        root,
        &producer("", "mise.toml", "."),
        &[],
        deadline,
    )
    .is_ok()
    {
        return Err(fail("preview discovery accepted an invalid task"));
    }
    if velnor_actions_mise::discover_preview_producer(
        root,
        &producer("build-preview", "sub/mise.toml", "other"),
        &[],
        deadline,
    )
    .is_ok()
    {
        return Err(fail("preview discovery accepted a mismatched source"));
    }
    if velnor_actions_mise::discover_preview_producer(
        root,
        &producer("build-preview", "mise.toml", "."),
        &[],
        deadline,
    )
    .is_ok()
    {
        return Err(fail("preview discovery accepted a missing source"));
    }
    Ok(())
}

fn check_channel_variants() -> Result<(), Box<dyn Error>> {
    if RustBinaryReleaseConfig::default().enabled() {
        return Err(fail("empty release config claims a channel"));
    }
    let mut preview_only = RustBinaryReleaseConfig::default();
    preview_only.preview.enabled = true;
    if !preview_only.enabled()
        || !preview_only.channel_enabled(PackageReleaseChannel::Preview)
        || preview_only.channel_enabled(PackageReleaseChannel::Stable)
    {
        return Err(fail("preview variant lost its channel"));
    }
    if RustBinaryReleaseConfig::default().preview.environment != "preview-publish"
        || RustBinaryReleaseConfig::default().stable.environment != "release"
    {
        return Err(fail("production variants drifted environments"));
    }
    let mut stable_only = RustBinaryReleaseConfig::default();
    stable_only.stable.enabled = true;
    if !stable_only.channel_enabled(PackageReleaseChannel::Stable)
        || stable_only.channel_enabled(PackageReleaseChannel::Preview)
    {
        return Err(fail("stable variant lost its channel"));
    }
    Ok(())
}

fn check_recipes() -> Result<(), Box<dyn Error>> {
    velnor_actions_orchestrator::validate_catalog()?;
    if velnor_actions_orchestrator::PREVIEW_PRODUCER_RECIPE_KEY != "preview.producer.task"
        || velnor_actions_orchestrator::PREVIEW_CACHE_RECIPE_KEY
            != velnor_actions_orchestrator::PREVIEW_PRODUCER_RECIPE_KEY
    {
        return Err(fail("preview recipe key drifted"));
    }
    let recipe = velnor_actions_orchestrator::lookup_recipe(
        velnor_actions_orchestrator::PREVIEW_PRODUCER_RECIPE_KEY,
    )
    .ok_or_else(|| fail("preview recipe missing"))?;
    if recipe.command_argv.is_some() {
        return Err(fail("preview producer gained a fixed command"));
    }
    Ok(())
}

fn check_observation_reports_unknown() -> Result<(), Box<dyn Error>> {
    let context = velnor_actions_contract::digest_b3(b"gc1-t40-observation-root");
    let mut session = ObservationSession::begin(
        "preview.producer.task",
        "preview-producer",
        &new_session_id(),
        &context,
    )?;
    let compiler = session.compiler_evidence(None, None, None, None, None, "gc1-t40 compiler")?;
    session.provide_compiler(compiler)?;
    let native = session.native_evidence(None, None, None, None, "gc1-t40 native")?;
    session.provide_native(native)?;
    let origin = session.origin_evidence(None, None, "gc1-t40 origin")?;
    session.provide_origin(origin)?;
    let cache = session.cache_evidence(None, "gc1-t40 cache")?;
    session.provide_cache(cache)?;
    let report: ObservationReport = session.finish(ChildOutcome::Success)?;
    if report.schema != 1 || report.child_failed {
        return Err(fail("preview observation misreported its outcome"));
    }
    if report.execution_context_digest != context {
        return Err(fail("preview observation lost its context"));
    }
    let measured = [
        &report.dependency_compilations,
        &report.application_compilations,
        &report.compiler_queries,
        &report.compiler_failed_calls,
        &report.compiler_interrupted_calls,
        &report.native_compilations,
        &report.native_queries,
        &report.native_failed_calls,
        &report.native_interrupted_calls,
        &report.origin_acquisitions,
        &report.origin_bytes,
        &report.cache_bytes,
    ];
    if measured
        .iter()
        .any(|value| value.value.is_some() || value.unknown_reason.is_none())
    {
        return Err(fail("preview observation replaced unknown with zero"));
    }
    if report.gaps.is_empty() {
        return Err(fail("preview observation dropped its gaps"));
    }
    let bytes = report.to_canonical_json()?;
    let parsed: serde_json::Value = serde_json::from_slice(&bytes)?;
    if parsed.get("schema").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(fail("preview observation lost its schema"));
    }
    Ok(())
}

fn check_observation_rejects() -> Result<(), Box<dyn Error>> {
    let context = velnor_actions_contract::digest_b3(b"gc1-t40-observation-root");
    if ObservationSession::begin("", "preview-producer", &new_session_id(), &context).is_ok() {
        return Err(fail("observation accepted an empty recipe"));
    }
    if ObservationSession::begin(
        "preview.producer.task",
        "preview-producer",
        &new_session_id(),
        "",
    )
    .is_ok()
    {
        return Err(fail("observation accepted an empty context"));
    }
    let mut session = ObservationSession::begin(
        "preview.producer.task",
        "preview-producer",
        &new_session_id(),
        &context,
    )?;
    if session
        .compiler_evidence(Some(1), None, None, None, None, "gc1-t40 partial")
        .is_ok()
    {
        return Err(fail("observation accepted partial dimensions"));
    }
    if session
        .native_evidence(Some(1), Some(0), None, None, "gc1-t40 partial")
        .is_ok()
    {
        return Err(fail("observation accepted partial native dimensions"));
    }
    let foreign = ObservationSession::begin(
        "preview.producer.task",
        "preview-producer",
        &new_session_id(),
        &context,
    )?;
    let compiler = foreign.compiler_evidence(None, None, None, None, None, "gc1-t40 compiler")?;
    if session.provide_compiler(compiler).is_ok() {
        return Err(fail("observation accepted a foreign session"));
    }
    let native = session.native_evidence(None, None, None, None, "gc1-t40 native")?;
    session.provide_native(native)?;
    let native_again = session.native_evidence(None, None, None, None, "gc1-t40 native")?;
    if session.provide_native(native_again).is_ok() {
        return Err(fail("observation accepted a duplicate dimension"));
    }
    if session.finish(ChildOutcome::Failure).is_ok() {
        return Err(fail("observation finished without dimensions"));
    }
    Ok(())
}

#[test]
fn gc1_t40_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_closure_union()?;
    check_closure_bound()?;
    check_channel_variants()?;
    check_recipes()?;
    check_observation_reports_unknown()?;
    Ok(())
}

#[test]
fn gc1_t40_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_closure_rejects()?;
    check_discovery_rejects()?;
    check_observation_rejects()?;
    Ok(())
}
