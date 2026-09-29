//! V1 fixed command vectors built only through the Mise adapter.

use std::ffi::OsString;

use velnor_actions_mise::{
    CandidateBuild, IsolatedCommand, PinnedTool, PinnedToolExec, RouteDriver, ToolCatalog,
    validate_exact_version,
};
use velnor_actions_rust::TaskGroup;
use velnor_actions_rust::tasks::cargo_payload_argv;
use velnor_actions_workflow_renderer::render::CandidateSpec;

use crate::OrchestratorError;
use crate::qualify::QualifyRequest;

/// Qualified cargo-deny release.
/// Source: `https://crates.io/api/v1/crates/cargo-deny`; checked 2026-09-29.
/// The mise registry shorthand `cargo-deny` resolves it (aqua backend); the
/// isolated `mise exec cargo-deny@0.20.2 -- cargo deny --version` probe
/// reported cargo-deny 0.20.2.
const CARGO_DENY_VERSION: &str = "0.20.2";

/// Qualified cargo-machete release.
/// Source: `https://crates.io/api/v1/crates/cargo-machete`; checked 2026-09-29.
/// The mise registry has no `cargo-machete` shorthand and the aqua registry
/// has no package, so the spec is backend-qualified `ubi:` (same precedent
/// as Nextest's aqua path): `mise ls-remote ubi:bnjbvr/cargo-machete` lists
/// 0.9.2 and the isolated `mise exec ubi:bnjbvr/cargo-machete@0.9.2 --
/// cargo machete --version` probe reported 0.9.2.
const CARGO_MACHETE_VERSION: &str = "0.9.2";

/// Mise tool specs the policy vectors may select, without versions.
const POLICY_TOOL_SPECS: [&str; 2] = ["cargo-deny", "ubi:bnjbvr/cargo-machete"];

/// Product crates scanned by the machete vector, in contract order.
///
/// Fixed paths keep the intentional `fixtures/symlink-escape` negative
/// fixture out of the scan: a bare `cargo machete` walk errors on the
/// fixture's dangling `src` symlink instead of skipping it.
const MACHETE_SCAN_CRATES: [&str; 7] = [
    "crates/velnor-actions-contract",
    "crates/velnor-actions-rust",
    "crates/velnor-actions-mise",
    "crates/velnor-actions-actionlint",
    "crates/velnor-actions-workflow-renderer",
    "crates/velnor-actions-orchestrator",
    "crates/velnor-actions-cli",
];

/// Contract-fixed display name of the policy zizmor step.
pub(crate) const ZIZMOR_STEP_NAME: &str = "Run zizmor";

/// V1 fixed vector for one group: pinned `mise` payload plus kind args.
///
/// The payload program follows the group's compile route: MBX profiles
/// execute through `mbx`, every other spelling through `cargo`.
pub(crate) fn task_argv(
    group: &TaskGroup,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let driver = RouteDriver::from_compile_driver(&group.compile_driver);
    let tools = driver.map_or(vec![PinnedTool::Rust], RouteDriver::tools);
    let program = OsString::from(driver.map_or("cargo", RouteDriver::program));
    let exec = PinnedToolExec::new(tools, &program, cargo_payload_argv(group)).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed policy-job vector: a pinned `gh` version probe.
pub(crate) fn verify_tools_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    let program = OsString::from("gh");
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Gh],
        &program,
        vec![OsString::from("--version")],
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed policy-job vector: `cargo deny --locked check` through pinned Mise.
pub(crate) fn deny_argv() -> Result<Vec<String>, OrchestratorError> {
    policy_argv(
        "cargo-deny",
        CARGO_DENY_VERSION,
        "cargo",
        &["deny", "--locked", "check"],
    )
}

/// Policy zizmor scan target: generated workflows only.
///
/// Never the repo root: `fixtures/` carries intentional negative
/// workflows that must fail adapter tests, not the policy audit.
const ZIZMOR_POLICY_INPUT: &str = ".github/workflows";

/// Policy zizmor config: the committed reviewed-tag exception file.
///
/// The config carries exactly the version-policy §2 `unpinned-uses`
/// ignore; a missing file errors the scan instead of silently dropping
/// the exception.
const ZIZMOR_POLICY_CONFIG: &str = ".zizmor.yml";

/// Fixed policy-job vector: offline zizmor audit through pinned Mise.
pub(crate) fn zizmor_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    let program = OsString::from("zizmor");
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Zizmor],
        &program,
        vec![
            OsString::from("--no-online-audits"),
            OsString::from("--config"),
            OsString::from(ZIZMOR_POLICY_CONFIG),
            OsString::from(ZIZMOR_POLICY_INPUT),
        ],
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed policy-job vector: `cargo machete` over product crates via Mise.
pub(crate) fn machete_argv() -> Result<Vec<String>, OrchestratorError> {
    let mut args = vec!["machete"];
    args.extend(MACHETE_SCAN_CRATES);
    policy_argv(
        "ubi:bnjbvr/cargo-machete",
        CARGO_MACHETE_VERSION,
        "cargo",
        &args,
    )
}

/// One policy vector: an allowlisted tool spec plus a fixed cargo payload.
///
/// Built through the Mise adapter's isolated `exec` constructor, so the
/// emitted shape (global flags, spec, `--` separator, payload) matches the
/// typed `PinnedToolExec` vectors byte for byte. The spec name must be
/// allowlisted and the version an exact pin; anything else fails closed.
fn policy_argv(
    spec: &str,
    version: &str,
    program: &str,
    args: &[&str],
) -> Result<Vec<String>, OrchestratorError> {
    if !POLICY_TOOL_SPECS.contains(&spec) {
        return Err(OrchestratorError::Contract {
            problem: format!("policy_tool_rejected:{spec}"),
        });
    }
    validate_exact_version(spec, version).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let payload: Vec<OsString> = [program]
        .into_iter()
        .chain(args.iter().copied())
        .map(OsString::from)
        .collect();
    let exec =
        IsolatedCommand::mise_exec(&[format!("{spec}@{version}")], &payload).map_err(|err| {
            OrchestratorError::Contract {
                problem: err.to_string(),
            }
        })?;
    strings_of(exec.argv()).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed pre-seed MBX route probe through pinned Mise.
///
/// Runs `mbx --version` under the exact pinned `mr-boxington` spec so
/// the verify step proves the compile route, not just the output file.
/// Resolves through Mise on every run, cold or warm.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the vector.
pub(crate) fn mbx_probe_argv(catalog: &ToolCatalog) -> Result<Vec<String>, OrchestratorError> {
    let program = OsString::from("mbx");
    let exec = PinnedToolExec::new(
        vec![PinnedTool::MrBoxington],
        &program,
        vec![OsString::from("--version")],
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed bootstrap §4 build vector through pinned Mise.
///
/// Shared by the candidate build and the pre-seed helper build, so both
/// compile `velnor-actions-cli`/`velnor-actions` with the exact same
/// pinned Rust plus MBX toolchain and flags.
pub(crate) fn candidate_build_argv(
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let build = CandidateBuild::new().map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    strings_of(build.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

/// Fixed candidate build plus qualification vectors.
pub(crate) fn candidate_spec(catalog: &ToolCatalog) -> Result<CandidateSpec, OrchestratorError> {
    Ok(CandidateSpec {
        build: candidate_build_argv(catalog)?,
        qualify: QualifyRequest::staged().argv()?,
    })
}

/// Convert fixed argv to UTF-8 strings.
fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(argv.len());
    for arg in argv {
        match arg.into_string() {
            Ok(text) => out.push(text),
            Err(_) => return Err("non_utf8_argv".to_owned()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_vectors_pin_specs_and_payloads() {
        let deny = deny_argv().expect("deny argv");
        let want: Vec<String> = [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "cargo-deny@0.20.2",
            "--",
            "cargo",
            "deny",
            "--locked",
            "check",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(deny, want);
        let machete = machete_argv().expect("machete argv");
        let want: Vec<String> = [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "ubi:bnjbvr/cargo-machete@0.9.2",
            "--",
            "cargo",
            "machete",
            "crates/velnor-actions-contract",
            "crates/velnor-actions-rust",
            "crates/velnor-actions-mise",
            "crates/velnor-actions-actionlint",
            "crates/velnor-actions-workflow-renderer",
            "crates/velnor-actions-orchestrator",
            "crates/velnor-actions-cli",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(machete, want);
        assert!(policy_argv("evil-tool", "1.2.3", "cargo", &["deny"]).is_err());
        assert!(policy_argv("cargo-deny", "latest", "cargo", &["deny"]).is_err());
    }

    #[test]
    fn mbx_probe_vector_is_byte_exact() {
        let probe = mbx_probe_argv(&ToolCatalog::pinned()).expect("probe argv");
        let want: Vec<String> = [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "mr-boxington@1.19.0",
            "--",
            "mbx",
            "--version",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(probe, want);
    }

    /// Minimal group with one compile-driver spelling.
    fn group_with_driver(driver: &str) -> TaskGroup {
        TaskGroup {
            task_id: "stack/rust|task/t".to_owned(),
            package_id: String::new(),
            package_name: String::new(),
            manifest_key: "root".to_owned(),
            kind: velnor_actions_rust::TaskKind::Clippy,
            configuration: "default".to_owned(),
            features: Vec::new(),
            target: "host".to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: None,
            compile_driver: driver.to_owned(),
            test_runner: "cargo_test".to_owned(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
        }
    }

    #[test]
    fn task_payload_program_follows_route_driver() {
        let catalog = ToolCatalog::pinned();
        for (driver, program, mbx) in [
            ("cargo", "cargo", false),
            ("mbx", "mbx", true),
            ("bogus", "cargo", false),
        ] {
            let argv = task_argv(&group_with_driver(driver), &catalog).expect("task argv");
            let at = argv.iter().position(|arg| arg == "--").expect("separator");
            assert_eq!(argv[at + 1], program, "{driver} program");
            assert_eq!(
                argv.iter().any(|arg| arg.contains("mr-boxington")),
                mbx,
                "{driver} tools"
            );
        }
    }

    #[test]
    fn zizmor_vector_is_pinned_and_offline() {
        let argv = zizmor_argv(&ToolCatalog::pinned()).expect("zizmor argv");
        assert_eq!(
            argv.join(" "),
            "mise --no-config --no-env --no-hooks exec zizmor@1.30.1 -- zizmor \
             --no-online-audits --config .zizmor.yml .github/workflows"
        );
    }

    #[test]
    fn section4_build_vector_is_byte_exact() {
        let build = candidate_build_argv(&ToolCatalog::pinned()).expect("build argv");
        let want: Vec<String> = [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
            "mr-boxington@1.19.0",
            "--",
            "mbx",
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(build, want);
    }

    #[test]
    fn candidate_build_delegates_to_mise_constructor() {
        let catalog = ToolCatalog::pinned();
        let mine = candidate_build_argv(&catalog).expect("build argv");
        let owned = CandidateBuild::new()
            .expect("mise build")
            .argv(&catalog)
            .into_iter()
            .map(|arg| arg.into_string().expect("utf8"))
            .collect::<Vec<_>>();
        assert_eq!(mine, owned);
    }
}
