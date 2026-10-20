//! Hard decision 5: tool versions come from the project's `mise.toml`.
//!
//! A version the project's Mise config declares never appears in generated
//! text; the generated resolver step reads it at run time, so bumping it in
//! `mise.toml` needs no regeneration. Tools the project does not declare
//! keep the Velnor pin.

use std::fs;

use velnor_actions_mise::{ACTIONLINT_VERSION, MR_BOXINGTON_VERSION, RUST_VERSION};
use velnor_actions_orchestrator::{plan_internal, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::RenderedTree;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use crate::impl_common::{TestResult, config_with_branch, make_repo};

/// Consumer config selecting the MBX driver and the Nextest runner.
const MBX_NEXTEST_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust]\ncompile_driver = \"mbx\"\ntest_runner = \"cargo_nextest\"\n";

/// Project config declaring Rust (generation inventory runs it locally,
/// so it is the generator pin), MBX, and Nextest at non-Velnor versions.
fn mise_toml(mbx: &str, nextest: &str) -> String {
    mise_toml_with_rust(RUST_VERSION, mbx, nextest)
}

fn mise_toml_with_rust(rust: &str, mbx: &str, nextest: &str) -> String {
    format!(
        "[tools]\nrust = \"{rust}\"\nmr-boxington = \"{mbx}\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"{nextest}\"\n"
    )
}

fn render(mbx: &str, nextest: &str) -> Result<RenderedTree, Box<dyn std::error::Error>> {
    let repo = make_repo(MBX_NEXTEST_CONFIG)?;
    fs::write(repo.path().join("mise.toml"), mise_toml(mbx, nextest))?;
    Ok(render_staged_tree(&prepare(repo.path())?)?)
}

fn texts(tree: &RenderedTree) -> Vec<(String, String)> {
    tree.files
        .iter()
        .map(|file| (file.path.clone(), file.bytes.clone()))
        .collect()
}

#[test]
fn project_declared_versions_never_appear_in_generated_text() -> TestResult {
    let tree = render("1.30.7", "0.9.977")?;
    for (path, text) in texts(&tree) {
        for literal in [
            "1.30.7",
            "0.9.977",
            &format!("rust@{RUST_VERSION}"),
            &format!("RUSTUP_TOOLCHAIN: {RUST_VERSION}"),
            &format!("{RUST_VERSION}-x86_64"),
        ] {
            assert!(
                !text.contains(literal),
                "{path} copies `{literal}`:\n{text}"
            );
        }
    }
    let ci = tree.get(WORKFLOW_PATH).ok_or("missing ci.yml")?;
    assert!(ci.contains("Resolve project tool versions"), "{ci}");
    assert!(ci.contains("velnor_tool VELNOR_TOOL_RUST '"), "{ci}");
    assert!(
        ci.contains("velnor_tool VELNOR_TOOL_MR_BOXINGTON '"),
        "{ci}"
    );
    assert!(ci.contains("velnor_tool VELNOR_TOOL_NEXTEST '"), "{ci}");
    // The resolver reads stored values only: no project config is loaded.
    assert!(
        ci.contains(r#"velnor_mise() { mise --no-config --no-env --no-hooks \"$@\"; }"#)
            && ci.contains("velnor_mise config get -f"),
        "{ci}"
    );
    assert!(
        ci.contains(r#"'rust@'\"${VELNOR_TOOL_RUST}\""#),
        "commands name the run-time reference: {ci}"
    );
    assert!(
        ci.contains("${{ env.VELNOR_TOOL_RUST }}"),
        "cache keys and action inputs read the resolved version: {ci}"
    );
    Ok(())
}

#[test]
fn bumping_project_versions_needs_no_regeneration() -> TestResult {
    let before = render("1.30.7", "0.9.977")?;
    let after = render("1.31.0", "0.9.978")?;
    assert_eq!(
        texts(&before),
        texts(&after),
        "a mise.toml version bump must not change any generated file"
    );
    Ok(())
}

fn plan_and_manifest(
    rust: &str,
    mbx: &str,
) -> Result<(serde_json::Value, serde_json::Value), Box<dyn std::error::Error>> {
    let repo = make_repo(MBX_NEXTEST_CONFIG)?;
    let root = repo.path();
    fs::write(
        root.join("mise.toml"),
        mise_toml_with_rust(rust, mbx, "0.9.977"),
    )?;
    crate::impl_common::git(&["add", "."], root)?;
    crate::impl_common::git(&["commit", "-m", "tool identity fixture"], root)?;
    let head = crate::impl_common::git_line(&["rev-parse", "HEAD"], root)?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": null,
        "head": head,
        "event": "push",
        "root": root.display().to_string(),
    });
    let response: serde_json::Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    let manifest_text = render_staged_tree(&prepare(root)?)?
        .get(".github/velnor/task-execution-manifest-v1.json")
        .ok_or("missing task execution manifest")?
        .to_owned();
    let (_, manifest_json) = manifest_text
        .split_once('\n')
        .ok_or("missing task manifest marker")?;
    let manifest: serde_json::Value = serde_json::from_str(manifest_json)?;
    Ok((response["plan"].clone(), manifest))
}

#[test]
fn project_tool_bumps_keep_static_manifest_binding_and_change_plan_identity() -> TestResult {
    let (first_plan, first_manifest) = plan_and_manifest(RUST_VERSION, "1.30.7")?;
    let (next_plan, next_manifest) = plan_and_manifest(RUST_VERSION, "1.31.0")?;
    let task_id = "stack/rust/root/nextest/default";
    let first_obligation = first_plan["obligations"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["task_id"] == task_id))
        .ok_or_else(|| format!("missing first plan task: {first_plan}"))?;
    let next_obligation = next_plan["obligations"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["task_id"] == task_id))
        .ok_or("missing next plan task")?;
    let first_record = &first_manifest["tasks"][task_id];
    let next_record = &next_manifest["tasks"][task_id];
    let matrix_toolchain = |plan: &serde_json::Value| {
        plan["matrix"]["include"]
            .as_array()?
            .iter()
            .find(|entry| entry["task_id"] == task_id)?
            .get("toolchain_id")
            .cloned()
    };

    assert_eq!(
        first_record["task_digest"],
        first_obligation["task_binding_digest"]
    );
    assert_eq!(
        next_record["task_digest"],
        next_obligation["task_binding_digest"]
    );
    assert_eq!(first_record["task_digest"], next_record["task_digest"]);
    assert_ne!(
        first_obligation["task_digest"],
        next_obligation["task_digest"]
    );
    assert_ne!(
        first_obligation["mbx_digest"],
        next_obligation["mbx_digest"]
    );
    assert_ne!(matrix_toolchain(&first_plan), matrix_toolchain(&next_plan));
    Ok(())
}

#[test]
fn undeclared_tools_keep_the_velnor_pin() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = tree.get(WORKFLOW_PATH).ok_or("missing ci.yml")?;
    assert!(ci.contains(&format!("rust@{RUST_VERSION}")), "{ci}");
    assert!(!ci.contains("VELNOR_TOOL_"), "{ci}");
    assert!(!ci.contains("Resolve project tool versions"), "{ci}");
    let declared = render(MR_BOXINGTON_VERSION, "0.9.977")?;
    let ci = declared.get(WORKFLOW_PATH).ok_or("missing ci.yml")?;
    // actionlint is velnor-internal here: the project declares no version.
    assert!(
        ci.contains(&format!("actionlint@{ACTIONLINT_VERSION}")),
        "{ci}"
    );
    // A declared tool is a reference even when it equals the Velnor pin.
    assert!(
        !ci.contains(&format!("mr-boxington@{MR_BOXINGTON_VERSION}")),
        "{ci}"
    );
    Ok(())
}

#[test]
fn declarations_outside_the_root_mise_toml_still_render_as_references() -> TestResult {
    // `.config/mise.toml`, `.tool-versions`, conf.d and `mise.<env>.toml`
    // are all read by Mise; none may be overridden by a Velnor pin.
    let repo = make_repo(MBX_NEXTEST_CONFIG)?;
    let root = repo.path();
    fs::create_dir_all(root.join(".config/mise/conf.d"))?;
    fs::write(
        root.join(".config/mise.toml"),
        format!("[tools]\nrust = \"{RUST_VERSION}\"\n"),
    )?;
    fs::write(
        root.join(".config/mise/conf.d/10-nextest.toml"),
        "[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.977\"\n",
    )?;
    fs::write(root.join(".tool-versions"), "mr-boxington 1.30.7\n")?;
    fs::write(
        root.join("mise.ci.toml"),
        "[tools]\nactionlint = \"1.7.11\"\n",
    )?;
    let tree = render_staged_tree(&prepare(root)?)?;
    let ci = tree.get(WORKFLOW_PATH).ok_or("missing ci.yml")?;
    for name in [
        "VELNOR_TOOL_RUST",
        "VELNOR_TOOL_MR_BOXINGTON",
        "VELNOR_TOOL_NEXTEST",
        "VELNOR_TOOL_ACTIONLINT",
    ] {
        assert!(
            ci.contains(&format!("velnor_tool {name} '")),
            "{name}: {ci}"
        );
    }
    assert!(!ci.contains("1.30.7") && !ci.contains("0.9.977"), "{ci}");
    assert!(!ci.contains("actionlint@1.7.11"), "{ci}");
    // The resolver reads every location Mise reads.
    for location in [
        ".config/mise.toml",
        ".config/mise/conf.d/*.toml",
        "mise.*.toml",
        ".tool-versions",
    ] {
        assert!(ci.contains(location), "{location}: {ci}");
    }
    Ok(())
}

#[test]
fn a_loose_project_version_fails_generation_naming_tool_and_file() -> TestResult {
    let repo = make_repo(MBX_NEXTEST_CONFIG)?;
    fs::write(
        repo.path().join("mise.toml"),
        "[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9\"\n",
    )?;
    let err = prepare(repo.path())
        .err()
        .ok_or("loose version must fail")?;
    let text = err.to_string();
    assert!(
        text.contains("project_tool_version_not_exact:tool=nextest,file=mise.toml,key=aqua:nextest-rs/nextest/cargo-nextest,value=0.9"),
        "{text}"
    );
    Ok(())
}

/// Each `Resolve project tool versions` step per job or composite of every
/// committed generated file.
fn resolver_steps_per_unit(text: &str) -> Vec<(String, usize)> {
    let marker = "- name: Resolve project tool versions";
    if !text.contains("\njobs:") {
        return vec![("composite".to_owned(), text.matches(marker).count())];
    }
    let mut units: Vec<(String, usize)> = Vec::new();
    let mut in_jobs = false;
    for line in text.lines() {
        if line == "jobs:" {
            in_jobs = true;
        } else if in_jobs && !line.starts_with(' ') && !line.is_empty() {
            in_jobs = false;
        } else if in_jobs
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.ends_with(':')
        {
            units.push((line.trim().trim_end_matches(':').to_owned(), 0));
        } else if line.trim_start().starts_with(marker)
            && let Some((_, count)) = units.last_mut()
        {
            *count += 1;
        }
    }
    units
}

#[test]
fn every_generated_unit_carries_at_most_one_resolver_step() -> TestResult {
    let github = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github");
    let mut files = Vec::new();
    for dir in ["workflows", "actions"] {
        let base = github.join(dir);
        for entry in fs::read_dir(&base)? {
            let path = entry?.path();
            if path.is_dir() {
                files.push(path.join("action.yml"));
            } else if path.extension().is_some_and(|ext| ext == "yml") {
                files.push(path);
            }
        }
    }
    let mut composites_with_resolver = 0;
    for file in files {
        for (unit, count) in resolver_steps_per_unit(&fs::read_to_string(&file)?) {
            assert!(
                count <= 1,
                "{} {unit}: {count} resolver steps",
                file.display()
            );
            composites_with_resolver += usize::from(
                count == 1 && file.file_name().is_some_and(|name| name == "action.yml"),
            );
        }
    }
    assert!(
        composites_with_resolver >= 1,
        "the generator-release composites read project versions"
    );
    Ok(())
}
