//! Preparation and execution share the named-check identity derivation.
use super::{TestResult, code_of, family_file, src_files};
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
    for path in src_files()? {
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
        ["execute.rs", "prepare.rs", "routing.rs"],
        "preparation, execution, and routing load configuration at their boundaries"
    );
    let internal = std::fs::read_to_string(family_file("internal.rs")?)?;
    assert!(internal.contains("prepare(&root)"), "plan runs preparation");
    let generate = std::fs::read_to_string(family_file("generate.rs")?)?;
    assert!(
        generate.contains("prep: &GenerationPreparation"),
        "generate consumes preparation"
    );
    let runtime = std::fs::read_to_string(family_file("execute.rs")?)?;
    let execution = runtime
        .split("pub fn execute_check_to(")
        .nth(1)
        .expect("production execution boundary")
        .split("fn run_check(")
        .next()
        .expect("execution body");
    assert!(execution.contains("velnor_actions_orchestrator_core::config::load_config(root)?"));
    assert!(execution.contains("velnor_actions_orchestrator_runtime_plan::binding::bind_check("));
    let binding = std::fs::read_to_string(family_file("binding.rs")?)?
        .split("pub fn bind_check(")
        .nth(1)
        .expect("production binding boundary")
        .split("pub fn parse_lane_variant(")
        .next()
        .expect("binding body")
        .to_owned();
    assert!(
        binding.contains("named_checks::plan::derive_lanes_until("),
        "runtime shares planner identity"
    );
    let planning = std::fs::read_to_string(family_file("internal/plan_obligation.rs")?)?;
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
