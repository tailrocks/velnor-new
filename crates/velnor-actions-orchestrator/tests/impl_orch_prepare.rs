//! Preparation and execution share the named-check identity derivation.
use super::{TestResult, code_of, orch_src, product_src_files};
/// Match the complete invoked identifier, excluding declarations and suffix names.
fn invokes(code: &str, name: &str) -> bool {
    if code.contains(&format!("fn {name}(")) {
        return false;
    }
    let token = format!("{name}(");
    code.match_indices(&token).any(|(position, _)| {
        position == 0 || !matches!(code.as_bytes()[position - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_')
    })
}
#[test]
fn exact_invocation_scan_excludes_neighbor_identifiers() {
    assert!(invokes("discover(&root, &config)?", "discover"));
    assert!(!invokes(
        "velnor_actions_mise::discover_checks(root, checks, catalog)?",
        "discover"
    ));
    assert!(!invokes("pub(crate) fn discover(root: &Path)", "discover"));
}
#[test]
fn plan_and_generate_share_one_prepare_path() -> TestResult {
    let mut discover_calls = Vec::new();
    let mut config_calls = Vec::new();
    for path in product_src_files()? {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.ends_with("_tests.rs") {
            continue;
        }
        for (_, code) in code_of(&path)? {
            if invokes(&code, "discover") {
                discover_calls.push(name.clone());
            }
            if invokes(&code, "load_config") {
                config_calls.push(name.clone());
            }
        }
    }
    assert_eq!(
        discover_calls,
        ["prepare.rs"],
        "full discovery belongs to preparation"
    );
    config_calls.sort();
    assert_eq!(
        config_calls,
        [
            "check_runtime.rs",
            "deps_lock.rs",
            "generate_unmanaged.rs",
            "owned_tool_preview.rs",
            "playwright_resolve.rs",
            "prepare.rs",
            "preview_producer_entry.rs",
            "preview_source_admission_finalize.rs",
            "private_fetch.rs",
            "qualification_resolver.rs",
            "routing.rs",
            "tap_prepare.rs",
            "tap_prepare_execute.rs",
        ],
        "preparation, execution, preview budget admission, routing, owned preview, qualification admission, captured tap preparation, the private dependency lock, the Playwright browser lock, and the private fetch load configuration at their boundaries"
    );
    Ok(())
}

#[test]
fn preparation_and_runtime_share_named_check_identity() -> TestResult {
    let internal = std::fs::read_to_string(orch_src().join("internal.rs"))?;
    assert!(internal.contains("prepare(&root)"), "plan runs preparation");
    let generate = std::fs::read_to_string(orch_src().join("generate.rs"))?;
    assert!(
        generate.contains("prep: &GenerationPreparation"),
        "generate consumes preparation"
    );
    let runtime = std::fs::read_to_string(orch_src().join("check_runtime.rs"))?;
    let execution = runtime
        .split("pub(crate) fn execute_check_to(")
        .nth(1)
        .expect("production execution boundary")
        .split("fn bind_check(")
        .next()
        .expect("execution body");
    assert!(execution.contains("crate::config::load_config(root)?"));
    assert!(
        execution.contains("bind_check(root, &item, &plan, entry, task_id, &catalog, deadline)?")
    );
    let binding = runtime
        .split("fn bind_check(")
        .nth(1)
        .expect("production binding boundary")
        .split("fn run_check(")
        .next()
        .expect("binding body");
    assert!(
        binding.contains("named_checks::plan::derive_lanes_until("),
        "runtime shares planner identity"
    );
    let planning = std::fs::read_to_string(orch_src().join("plan_obligation.rs"))?;
    let group = planning
        .split("pub(crate) fn plan_group(")
        .nth(1)
        .expect("production planning boundary")
        .split("fn complete_group(")
        .next()
        .expect("planning body");
    assert!(
        group.contains("named_checks::plan::derive_lanes("),
        "named obligations share runtime derivation"
    );
    Ok(())
}

#[test]
fn preview_admission_bypasses_shared_preparation_while_stable_retains_it() -> TestResult {
    let mut callers = Vec::new();
    for path in product_src_files()? {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.ends_with("_tests.rs") {
            continue;
        }
        for (_, code) in code_of(&path)? {
            if invokes(&code, "prepare_with_config") {
                callers.push(name.to_string());
            }
        }
    }
    assert_eq!(callers, ["preview_producer_entry.rs"]);
    let entry = std::fs::read_to_string(orch_src().join("preview_producer_entry.rs"))?;
    assert_eq!(
        entry
            .matches("crate::config::load_config(&workspace)?")
            .count(),
        1
    );
    assert!(entry.contains("producer_deadline(started, minutes)?"));
    let compact_entry = entry.split_whitespace().collect::<String>();
    assert!(compact_entry.contains("deadline.scope(||"));
    assert!(entry.contains("admitted_policy_plan(&temp, deadline)?"));
    assert_eq!(entry.matches("prepare_with_config(").count(), 1);
    let compact_entry = entry.split_whitespace().collect::<String>();
    let preview = compact_entry
        .split_once("PackageReleaseChannel::Preview=>{")
        .and_then(|(_, tail)| {
            tail.split_once("PackageReleaseChannel::Stable=>{")
                .map(|pair| pair.0)
        })
        .expect("Preview branch precedes Stable");
    assert!(preview.contains("preflight_preview_source("));
    assert!(preview.contains("context::derive(&workspace,&policy,plan,deadline)?"));
    assert!(preview.contains("execution::execute("));
    assert!(preview.contains("deadline"));
    assert!(
        !preview.contains("prepare_with_config(")
            && !preview.contains("binary_preview_emit::release_policy"),
        "Preview uses its authenticated A admission without repository metadata"
    );
    let stable = compact_entry
        .split_once("PackageReleaseChannel::Stable=>{")
        .map(|(_, stable)| stable)
        .expect("Stable branch remains explicit");
    assert!(stable.contains("prepare_with_config(&workspace,config)?"));
    assert!(stable.contains("source_preview(&prep,&policy,&plan)?"));
    assert!(stable.contains("execution::execute("));
    assert!(stable.contains("deadline"));
    assert!(!entry.contains("let mut config"));
    let prepare = std::fs::read_to_string(orch_src().join("prepare.rs"))?;
    assert!(prepare.contains("prepare_inner(root, None, None)"));
    assert!(prepare.contains("prepare_inner(root, Some(phases), None)"));
    assert!(prepare.contains("prepare_inner(root, None, Some(config))"));
    let supplied = prepare
        .split("pub(crate) fn prepare_with_config(")
        .nth(1)
        .expect("supplied config boundary")
        .split("pub(crate) fn prepare_with_phase_timings(")
        .next()
        .expect("shared preparation boundary");
    assert!(supplied.contains(".validate(crate::config::CONFIG_REL)"));
    assert!(supplied.contains("crate::config::check_policy_mode(&config)?"));
    let shared = prepare
        .split("fn prepare_inner(")
        .nth(1)
        .expect("shared pipeline");
    assert!(
        shared
            .find("match admitted_config")
            .expect("config convergence")
            < shared.find("resolve_tool_catalog").expect("catalog")
    );
    Ok(())
}
