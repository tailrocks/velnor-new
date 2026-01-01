//! G5-residual: per-job install sets pin which jobs carry the trio.
use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_mise::{
    IsolatedCommand, MiseInstall, PinnedTool, PreparePinnedTools, ToolCatalog, ToolHomes,
};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use crate::impl_common::{TestResult, fixture_manifest_json, git, without_ambient_identity};

/// Velnor-policy workspace: a validator-spawning suite plus a plain crate.
pub(super) fn velnor_workspace() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
        root,
    )?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"testmain\"\n",
    )?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(repo_policy_path(root), repo_policy()?)?;
    fs::write(root.join(".velnor/generator.lock"), lock_text()?)?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\n  \"crates/velnor-actions-cli\",\n  \"crates/demo\",\n]\n",
    )?;
    for name in ["velnor-actions-cli", "demo"] {
        let dir = root.join("crates").join(name);
        fs::create_dir_all(dir.join("src"))?;
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )?;
        fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    Ok(dir)
}

/// Version policy mirrored from the working tree (velnor prepare needs it).
fn repo_policy() -> Result<String, Box<dyn std::error::Error>> {
    let path = format!(
        "{}/../../.velnor/version-policy.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    Ok(fs::read_to_string(path)?)
}

fn repo_policy_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".velnor/version-policy.toml")
}

/// Three-target lock accepted by the provenance seed gate (F3: with commit).
fn lock_text() -> Result<String, Box<dyn std::error::Error>> {
    use std::fmt::Write as _;
    let version = env!("CARGO_PKG_VERSION");
    let mut bins = String::new();
    for target in velnor_actions_contract::SUPPORTED_TARGETS {
        write!(
            bins,
            "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r/{target}\"\nsha256 = \"{}\"\n",
            "a".repeat(64)
        )?;
    }
    Ok(format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"2026.10.6\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        "e".repeat(40),
        "b".repeat(64)
    ))
}

/// `Prepare pinned tools` run lines per job id from rendered YAML.
fn prepare_runs(yaml: &str) -> BTreeMap<String, String> {
    let mut runs = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut lines = yaml.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':') {
            current = Some(line.trim().trim_end_matches(':').to_owned());
        } else if line.trim() == "- name: Prepare pinned tools" {
            let mut run = String::new();
            for next in lines.by_ref() {
                let trimmed = next.trim_start();
                if trimmed.starts_with("run: ") {
                    run.push_str(trimmed.trim_start_matches("run: "));
                    break;
                }
                if trimmed.starts_with("- name: ") || trimmed.starts_with("uses: ") {
                    break;
                }
            }
            if let Some(id) = current.clone() {
                runs.insert(id, run);
            }
        }
    }
    runs
}

/// Extract a rendered job through the next peer job.
fn job_section<'a>(yaml: &'a str, id: &str) -> Option<&'a str> {
    let prefix = format!("  {id}:\n");
    let start = yaml.find(&prefix)? + prefix.len();
    let tail = &yaml[start..];
    let mut end = 0;
    for line in tail.split_inclusive('\n') {
        if line.starts_with("  ") && !line.starts_with("   ") {
            break;
        }
        end += line.len();
    }
    Some(&tail[..end])
}

/// Validators invoke their exact analyzer pin through isolated Mise exec.
fn assert_pinned_exec(section: &str, tool: &str) -> Result<(), String> {
    if section.contains("mise --no-config --no-env --no-hooks exec") && section.contains(tool) {
        Ok(())
    } else {
        Err(format!(
            "validator does not execute pinned {tool}: {section}"
        ))
    }
}

/// Cache miss remains executable: explicit install sits after setup and before exec.
fn assert_cold_install_order(section: &str, command: &str) -> Result<(), String> {
    let position = |name: &str| {
        section
            .find(name)
            .ok_or_else(|| format!("job misses {name}: {section}"))
    };
    let restore = position("- name: Restore Mise tools")?;
    let setup = position("- name: Setup Mise")?;
    let install = position("- name: Prepare pinned tools")?;
    let run = position(command)?;
    if !(restore < setup && setup < install && install < run) {
        return Err(format!("cold install order is wrong: {section}"));
    }
    Ok(())
}

#[test]
fn velnor_jobs_carry_trio_only_where_executed() -> TestResult {
    without_ambient_identity("velnor_jobs_carry_trio_only_where_executed", || {
        let repo = velnor_workspace()?;
        let prep = prepare(repo.path())?;
        let tree = render_staged_tree(&prep)?;
        let yaml = tree
            .get(WORKFLOW_PATH)
            .ok_or("rendered workflow missing")?
            .to_owned();
        let runs = prepare_runs(&yaml);
        let trio = ["actionlint@", "shellcheck@", "zizmor@"];
        let plan = runs.get("plan").ok_or("plan must prepare tools")?;
        for spec in trio {
            assert!(plan.contains(spec), "plan installs {spec}: {plan}");
        }
        let cli = runs
            .get("rust-velnor-actions-cli")
            .ok_or("cli crate job missing")?;
        for spec in trio {
            assert!(cli.contains(spec), "cli suite spawns validators: {cli}");
        }
        let demo = runs.get("rust-demo").ok_or("demo crate job missing")?;
        for spec in trio {
            assert!(!demo.contains(spec), "demo must trim {spec}: {demo}");
        }
        assert!(demo.contains("rust@"), "demo keeps its driver: {demo}");
        let required = runs.get("required").ok_or("required missing")?;
        assert!(required.contains("gh@"), "required installs gh: {required}");
        for spec in trio {
            assert!(!required.contains(spec), "required must not carry {spec}");
        }
        let lint = runs.get("actionlint").ok_or("lint must prepare tools")?;
        assert!(lint.contains("actionlint@1.7.12"), "{lint}");
        assert!(lint.contains("shellcheck@0.11.0"), "{lint}");
        let machete = runs.get("cargo-machete").ok_or("machete must prepare")?;
        assert!(
            machete.contains("http:cargo-machete[url=https://github.com/bnjbvr/cargo-machete/releases/download/v0.9.2/cargo-machete-v0.9.2-x86_64-unknown-linux-musl.tar.gz,checksum=sha256:48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9]@0.9.2"),
            "{machete}"
        );
        let zizmor = runs.get("zizmor").ok_or("zizmor must prepare")?;
        assert!(zizmor.contains("zizmor@1.30.1"), "{zizmor}");
        for (id, tool, command) in [
            ("actionlint", "actionlint@1.7.12", "Run actionlint"),
            (
                "cargo-machete",
                "http:cargo-machete[url=https://github.com/bnjbvr/cargo-machete/releases/download/v0.9.2/cargo-machete-v0.9.2-x86_64-unknown-linux-musl.tar.gz,checksum=sha256:48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9]@0.9.2",
                "Run cargo-machete",
            ),
            ("zizmor", "zizmor@1.30.1", "Run zizmor"),
        ] {
            let section = job_section(&yaml, id).ok_or("validator job missing")?;
            assert_pinned_exec(section, tool)?;
            assert_cold_install_order(section, command)?;
        }
        for id in ["alint", "cargo-deny"] {
            assert!(
                !runs.contains_key(id),
                "{id} keeps its own install boundary"
            );
        }
        Ok(())
    })
}

/// No emitted install argv lacks `--no-config`: every typed install
/// constructor plus every `mise ... install` run line in rendered YAML
/// (Prepare steps and the deny `sh -c` script alike). One missing flag
/// would load repo config, so the scan fails on the first gap.
#[test]
fn every_emitted_install_argv_carries_no_config() -> TestResult {
    without_ambient_identity("every_emitted_install_argv_carries_no_config", || {
        let catalog = ToolCatalog::pinned();
        let specs = vec!["rust@1.98.1".to_owned()];
        let install = IsolatedCommand::mise_install(&specs).map_err(|err| err.to_string())?;
        let request = MiseInstall::new(vec![PinnedTool::Rust]).map_err(|err| err.to_string())?;
        let step = PreparePinnedTools::new(vec![PinnedTool::Rust], ToolHomes::runner_temp())
            .map_err(|err| err.to_string())?;
        for argv in [install.argv(), request.argv(&catalog), step.argv(&catalog)] {
            let tokens = argv_text(&argv);
            let at = tokens
                .iter()
                .position(|token| token == "install")
                .ok_or("typed install argv misses install")?;
            for flag in ["--no-config", "--no-env", "--no-hooks"] {
                if !tokens[..at].contains(&flag.to_owned()) {
                    return Err(format!("typed install argv misses {flag}: {tokens:?}").into());
                }
            }
        }
        let repo = velnor_workspace()?;
        let prep = prepare(repo.path())?;
        let yaml = velnor_actions_workflow_renderer::render_workflow_ir(
            &prep.workflow.ir,
            prep.config.workflow.policy,
            prep.workflow.support.as_ref(),
            &prep.workflow.context,
        )
        .map_err(|err| format!("render: {err}"))?;
        let mut installs = 0;
        let mut saw_deny_install = false;
        for line in yaml.lines() {
            if line.trim_start().starts_with("run:") {
                let found = check_run_line_installs(line)?;
                installs += found;
                if found > 0 && line.contains("cargo-deny@") {
                    saw_deny_install = true;
                }
            }
        }
        if installs < 5 {
            return Err(format!("expected Prepare installs plus deny, saw {installs}").into());
        }
        if !saw_deny_install {
            return Err("deny sh -c install missing from scan".into());
        }
        Ok(())
    })
}

/// Lossy text of one argv vector.
fn argv_text(argv: &[std::ffi::OsString]) -> Vec<String> {
    argv.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// Count `mise ... install` vectors on one run line, failing closed when
/// any lacks the isolation trio between `mise` and `install`.
fn check_run_line_installs(line: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut installs = 0;
    for (at, token) in tokens.iter().enumerate() {
        if *token != "install" {
            continue;
        }
        let Some(mise_at) = tokens[..at].iter().rposition(|found| *found == "mise") else {
            continue;
        };
        installs += 1;
        for flag in ["--no-config", "--no-env", "--no-hooks"] {
            if !tokens[mise_at..at].contains(&flag) {
                return Err(format!("install without {flag}: {line}").into());
            }
        }
    }
    Ok(installs)
}

/// The deny ambient install is audited, not invisible: with no lock
/// committed, the pinned `cargo-deny` spec lands in the missing-lock
/// advisory by name instead of passing silently.
#[test]
fn deny_install_is_audited() -> TestResult {
    without_ambient_identity("deny_install_is_audited", || {
        let repo = velnor_workspace()?;
        let prep = prepare(repo.path())?;
        assert!(prep.lock_audit_blocking.is_empty());
        let summary = prep
            .discovery
            .recommendations
            .iter()
            .find(|line| line.contains("tool_install_unverified"))
            .ok_or("missing-lock summary")?;
        assert!(summary.contains("cargo-deny@0.20.2"), "{summary}");
        Ok(())
    })
}
